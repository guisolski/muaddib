use crate::core::citations::normalize_url;
use crate::core::config::WebSearchConfig;
use crate::core::mode::Mode;
use crate::core::plan::SearchPlan;
use crate::core::rank::{pool_budget, rank_hits};
use crate::core::websearch::{
    WebEngineSpec, WebHit, engines_for_mode, fallback_engine, resolve_url, web_query,
};
use crate::pipeline::SearchEvent;
use futures::stream::{self, StreamExt};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub const WEB_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const WEB_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const PAGE_FETCH_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
const CONCURRENT_WEB_QUERIES: usize = 2;

pub type BoxedHitsFuture<'a> = Pin<Box<dyn Future<Output = Vec<WebHit>> + Send + 'a>>;
pub type BoxedPageFuture<'a> = Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;

pub trait WebFetcher: Send + Sync {
    fn search<'a>(
        &'a self,
        spec: &'static WebEngineSpec,
        base_url: &'a str,
        query: &'a str,
        mailto: &'a str,
        max_hits: usize,
    ) -> BoxedHitsFuture<'a>;

    fn fetch_page<'a>(&'a self, _url: &'a str) -> BoxedPageFuture<'a> {
        Box::pin(async { None })
    }
}

pub struct NoopWebFetcher;

impl WebFetcher for NoopWebFetcher {
    fn search<'a>(
        &'a self,
        _spec: &'static WebEngineSpec,
        _base_url: &'a str,
        _query: &'a str,
        _mailto: &'a str,
        _max_hits: usize,
    ) -> BoxedHitsFuture<'a> {
        Box::pin(async { Vec::new() })
    }
}
pub struct HttpWebFetcher {
    client: Option<reqwest::Client>,
}
impl HttpWebFetcher {
    pub fn new() -> Self {
        Self {
            client: crate::pipeline::http::build_client(),
        }
    }
}
impl Default for HttpWebFetcher {
    fn default() -> Self {
        Self::new()
    }
}
impl WebFetcher for HttpWebFetcher {
    fn search<'a>(
        &'a self,
        spec: &'static WebEngineSpec,
        base_url: &'a str,
        query: &'a str,
        mailto: &'a str,
        max_hits: usize,
    ) -> BoxedHitsFuture<'a> {
        Box::pin(async move {
            match &self.client {
                Some(client) => fetch_hits(client, spec, base_url, query, mailto, max_hits)
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        })
    }

    fn fetch_page<'a>(&'a self, url: &'a str) -> BoxedPageFuture<'a> {
        Box::pin(async move {
            match &self.client {
                Some(client) => fetch_page_body(client, url).await,
                None => None,
            }
        })
    }
}
async fn fetch_page_body(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client
        .get(url)
        .timeout(PAGE_FETCH_TIMEOUT)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let html_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.starts_with("text/html") || value.starts_with("application/xhtml")
        });
    if !html_type {
        return None;
    }
    let too_large = response
        .content_length()
        .is_some_and(|length| length > MAX_PAGE_BYTES as u64);
    if too_large {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() <= MAX_PAGE_BYTES).then(|| String::from_utf8_lossy(&bytes).into_owned())
}
async fn fetch_hits(
    client: &reqwest::Client,
    spec: &'static WebEngineSpec,
    base_url: &str,
    query: &str,
    mailto: &str,
    max_hits: usize,
) -> Option<Vec<WebHit>> {
    use crate::core::websearch::{RequestShape, WebCategory, encoded_params, parse_hits};
    let encoded = encoded_params(spec, query, mailto);
    let request = match spec.shape {
        RequestShape::Get => client.get(format!("{base_url}?{encoded}")),
        RequestShape::PostForm => client
            .post(base_url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(encoded),
    };
    let request = match spec.category {
        WebCategory::Academic => request.header(reqwest::header::ACCEPT, "application/json"),
        WebCategory::Web => request,
    };
    let response = request.timeout(WEB_REQUEST_TIMEOUT).send().await.ok()?;
    let body = response.text().await.ok()?;
    Some(parse_hits(spec, &body, max_hits))
}

pub fn default_fetcher() -> Arc<dyn WebFetcher> {
    Arc::new(HttpWebFetcher::new())
}

pub async fn websearch_stage(
    fetcher: &dyn WebFetcher,
    plan: &SearchPlan,
    config: &WebSearchConfig,
    tx: &Sender<SearchEvent>,
) -> Vec<Vec<WebHit>> {
    let engines = if config.enabled {
        engines_for_mode(plan.mode, config)
    } else {
        Vec::new()
    };
    if engines.is_empty() {
        return vec![Vec::new(); plan.sub_queries.len()];
    }
    let query_futures: Vec<_> = plan
        .sub_queries
        .iter()
        .map(|sub| query_hits(fetcher, &engines, &sub.query, plan.mode, config))
        .collect();
    let per_query: Vec<Vec<WebHit>> = stream::iter(query_futures)
        .buffered(CONCURRENT_WEB_QUERIES)
        .collect()
        .await;
    let count = per_query.iter().map(Vec::len).sum();
    let urls = consulted_urls(&per_query);
    let _ = tx.send(SearchEvent::WebHits { count, urls }).await;
    per_query
}

pub const MAX_REPORTED_URLS: usize = 30;

fn consulted_urls(per_query: &[Vec<WebHit>]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    per_query
        .iter()
        .flatten()
        .filter(|hit| seen.insert(normalize_url(&hit.url)))
        .map(|hit| hit.url.clone())
        .take(MAX_REPORTED_URLS)
        .collect()
}

async fn query_hits(
    fetcher: &dyn WebFetcher,
    engines: &[&'static WebEngineSpec],
    query: &str,
    mode: Mode,
    config: &WebSearchConfig,
) -> Vec<WebHit> {
    let deadline = tokio::time::Instant::now() + WEB_QUERY_TIMEOUT;
    let engine_query = web_query(query, mode);
    let budget = usize::from(config.max_hits_per_query);
    let pool_target = pool_budget(budget);
    let mut pool: Vec<WebHit> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for spec in engines {
        let remaining = pool_target.saturating_sub(pool.len());
        let time_left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining == 0 || time_left.is_zero() {
            break;
        }
        let Some(base_url) = resolve_url(spec, config) else {
            continue;
        };
        let engine_hits = tokio::time::timeout(
            time_left,
            engine_hits_with_fallback(
                fetcher,
                spec,
                &base_url,
                &engine_query,
                &config.mailto,
                remaining,
            ),
        )
        .await
        .unwrap_or_default();
        pool.extend(
            engine_hits
                .into_iter()
                .filter(|hit| seen.insert(normalize_url(&hit.url)))
                .take(remaining),
        );
    }
    rank_hits(query, &pool, budget)
}

async fn engine_hits_with_fallback(
    fetcher: &dyn WebFetcher,
    spec: &'static WebEngineSpec,
    base_url: &str,
    query: &str,
    mailto: &str,
    max_hits: usize,
) -> Vec<WebHit> {
    let primary = fetcher
        .search(spec, base_url, query, mailto, max_hits)
        .await;
    if primary.is_empty()
        && let Some(fallback) = fallback_engine(spec)
    {
        return fetcher
            .search(fallback, fallback.url, query, mailto, max_hits)
            .await;
    }
    primary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mode::Mode;
    use crate::core::plan::SubQuery;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    struct FakeFetcher {
        canned: BTreeMap<&'static str, Vec<WebHit>>,
        slow_engines: Vec<&'static str>,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeFetcher {
        fn with(canned: BTreeMap<&'static str, Vec<WebHit>>) -> Self {
            Self {
                canned,
                slow_engines: Vec::new(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn called(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl WebFetcher for FakeFetcher {
        fn search<'a>(
            &'a self,
            spec: &'static WebEngineSpec,
            _base_url: &'a str,
            _query: &'a str,
            _mailto: &'a str,
            max_hits: usize,
        ) -> BoxedHitsFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(spec.name);
                if self.slow_engines.contains(&spec.name) {
                    tokio::time::sleep(Duration::from_mins(10)).await;
                }
                self.canned
                    .get(spec.name)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .take(max_hits)
                    .collect()
            })
        }
    }

    fn hit(url: &str) -> WebHit {
        WebHit {
            title: format!("title of {url}"),
            url: url.to_string(),
            snippet: "snippet".to_string(),
            engine: "ddg",
            ..Default::default()
        }
    }

    fn plan(queries: &[&str]) -> SearchPlan {
        SearchPlan {
            original: "q".to_string(),
            mode: Mode::General,
            answer_lang: "en".to_string(),
            sub_queries: queries
                .iter()
                .map(|query| SubQuery {
                    query: (*query).to_string(),
                    lang: "en".to_string(),
                    rationale: String::new(),
                })
                .collect(),
        }
    }

    fn config(max_hits: u8) -> WebSearchConfig {
        WebSearchConfig {
            max_hits_per_query: max_hits,
            ..WebSearchConfig::default()
        }
    }

    async fn run_stage(
        fetcher: &FakeFetcher,
        plan: &SearchPlan,
        config: &WebSearchConfig,
    ) -> (Vec<Vec<WebHit>>, Vec<SearchEvent>) {
        let (tx, mut rx) = mpsc::channel(16);
        let hits = websearch_stage(fetcher, plan, config, &tx).await;
        drop(tx);
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        (hits, events)
    }

    #[tokio::test]
    async fn stage_returns_one_ordered_hit_list_per_sub_query() {
        let fetcher = FakeFetcher::with(BTreeMap::from([(
            "ddg",
            vec![hit("https://a.example/1"), hit("https://a.example/2")],
        )]));
        let (hits, events) = run_stage(&fetcher, &plan(&["first", "second"]), &config(5)).await;
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].len(), 2);
        assert_eq!(hits[0], hits[1]);
        assert_eq!(
            events,
            vec![SearchEvent::WebHits {
                count: 4,
                urls: vec![
                    "https://a.example/1".to_string(),
                    "https://a.example/2".to_string()
                ],
            }]
        );
    }

    #[tokio::test]
    async fn waterfall_stops_once_the_pool_budget_is_met() {
        let overfull: Vec<WebHit> = (1..=7)
            .map(|idx| hit(&format!("https://a.example/{idx}")))
            .collect();
        let fetcher = FakeFetcher::with(BTreeMap::from([("ddg", overfull)]));
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(2)).await;
        assert_eq!(hits[0].len(), 2);
        assert_eq!(fetcher.called(), vec!["ddg"]);
    }

    #[tokio::test]
    async fn relevant_hit_from_a_later_engine_outranks_earlier_filler() {
        let fetcher = FakeFetcher::with(BTreeMap::from([
            (
                "ddg",
                vec![WebHit {
                    title: "gardening tips".to_string(),
                    url: "https://garden.example".to_string(),
                    snippet: "flowers and soil".to_string(),
                    engine: "ddg",
                    ..Default::default()
                }],
            ),
            (
                "bing",
                vec![WebHit {
                    title: "rust tokio runtime deep dive".to_string(),
                    url: "https://tokio.example".to_string(),
                    snippet: "tokio internals explained".to_string(),
                    engine: "bing",
                    ..Default::default()
                }],
            ),
        ]));
        let (hits, _) = run_stage(&fetcher, &plan(&["rust tokio runtime"]), &config(1)).await;
        let urls: Vec<&str> = hits[0].iter().map(|hit| hit.url.as_str()).collect();
        assert_eq!(urls, vec!["https://tokio.example"]);
    }

    #[tokio::test]
    async fn empty_primary_engine_falls_back_before_moving_on() {
        let fetcher = FakeFetcher::with(BTreeMap::from([(
            "ddg-lite",
            vec![hit("https://lite.example/1")],
        )]));
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(1)).await;
        assert_eq!(hits[0][0].url, "https://lite.example/1");
        assert_eq!(fetcher.called(), vec!["ddg", "ddg-lite", "bing"]);
    }

    #[tokio::test]
    async fn duplicate_urls_across_engines_are_dropped() {
        let fetcher = FakeFetcher::with(BTreeMap::from([
            ("ddg", vec![hit("https://same.example/page")]),
            (
                "bing",
                vec![
                    hit("https://same.example/page/"),
                    hit("https://new.example"),
                ],
            ),
        ]));
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(5)).await;
        let urls: Vec<&str> = hits[0].iter().map(|hit| hit.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://same.example/page", "https://new.example"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_engines_time_out_keeping_partial_hits() {
        let fetcher = FakeFetcher {
            canned: BTreeMap::from([
                ("ddg", vec![hit("https://fast.example")]),
                ("bing", vec![hit("https://slow.example")]),
            ]),
            slow_engines: vec!["bing"],
            calls: Mutex::new(Vec::new()),
        };
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(5)).await;
        let urls: Vec<&str> = hits[0].iter().map(|hit| hit.url.as_str()).collect();
        assert_eq!(urls, vec!["https://fast.example"]);
    }

    #[tokio::test]
    async fn disabled_config_skips_fetching_entirely() {
        let fetcher = FakeFetcher::with(BTreeMap::from([("ddg", vec![hit("https://a.example")])]));
        let disabled = WebSearchConfig::disabled();
        let (hits, events) = run_stage(&fetcher, &plan(&["first", "second"]), &disabled).await;
        assert_eq!(hits, vec![Vec::new(), Vec::new()]);
        assert!(events.is_empty());
        assert!(fetcher.called().is_empty());
    }

    #[tokio::test]
    async fn unknown_allowlist_engines_disable_the_stage_gracefully() {
        let fetcher = FakeFetcher::with(BTreeMap::from([("ddg", vec![hit("https://a.example")])]));
        let config = WebSearchConfig {
            engines: vec!["altavista".to_string()],
            ..WebSearchConfig::default()
        };
        let (hits, events) = run_stage(&fetcher, &plan(&["only"]), &config).await;
        assert_eq!(hits, vec![Vec::new()]);
        assert!(events.is_empty());
        assert!(fetcher.called().is_empty());
    }

    async fn serve_page(body_len: usize, declare_length: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/page", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 4096];
            let _ = stream.read(&mut request).await;
            let length = if declare_length {
                format!("Content-Length: {body_len}\r\n")
            } else {
                String::new()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n{length}Connection: close\r\n\r\n"
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&vec![b'x'; body_len]).await;
            let _ = stream.flush().await;
        });
        url
    }

    #[tokio::test]
    async fn a_fetched_page_is_kept_up_to_two_megabytes_and_no_further() {
        const TWO_MIB: usize = 2_097_152;
        struct Case {
            name: &'static str,
            body_len: usize,
            declare_length: bool,
            want_kept: bool,
        }
        let cases = [
            Case {
                name: "a page of exactly two megabytes is kept",
                body_len: TWO_MIB,
                declare_length: true,
                want_kept: true,
            },
            Case {
                name: "one byte over the cap is refused from the declared length",
                body_len: TWO_MIB + 1,
                declare_length: true,
                want_kept: false,
            },
            Case {
                name: "one byte over the cap is refused after reading when no length is declared",
                body_len: TWO_MIB + 1,
                declare_length: false,
                want_kept: false,
            },
        ];
        let client = crate::pipeline::http::build_client().expect("the shared client builds");
        for case in cases {
            let url = serve_page(case.body_len, case.declare_length).await;
            let body = fetch_page_body(&client, &url).await;
            assert_eq!(body.is_some(), case.want_kept, "{}", case.name);
            if let Some(body) = body {
                assert_eq!(body.len(), case.body_len, "{}", case.name);
            }
        }
    }

    #[tokio::test]
    async fn a_fetched_page_that_is_not_html_is_dropped() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/page", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0_u8; 4096];
            let _ = stream.read(&mut request).await;
            let response = "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc";
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.flush().await;
        });
        let client = crate::pipeline::http::build_client().expect("the shared client builds");
        assert!(fetch_page_body(&client, &url).await.is_none());
    }
}
