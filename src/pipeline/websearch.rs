use crate::core::citations::normalize_url;
use crate::core::config::WebSearchConfig;
use crate::core::plan::SearchPlan;
use crate::core::websearch::{WebEngineSpec, WebHit, engines_for_mode, fallback_engine};
use crate::pipeline::SearchEvent;
use futures::stream::{self, StreamExt};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub const WEB_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(feature = "websearch")]
const WEB_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const CONCURRENT_WEB_QUERIES: usize = 2;

pub type BoxedHitsFuture<'a> = Pin<Box<dyn Future<Output = Vec<WebHit>> + Send + 'a>>;

pub trait WebFetcher: Send + Sync {
    fn search<'a>(
        &'a self,
        spec: &'static WebEngineSpec,
        query: &'a str,
        mailto: &'a str,
        max_hits: usize,
    ) -> BoxedHitsFuture<'a>;
}

pub struct NoopWebFetcher;

impl WebFetcher for NoopWebFetcher {
    fn search<'a>(
        &'a self,
        _spec: &'static WebEngineSpec,
        _query: &'a str,
        _mailto: &'a str,
        _max_hits: usize,
    ) -> BoxedHitsFuture<'a> {
        Box::pin(async { Vec::new() })
    }
}

#[cfg(feature = "websearch")]
pub struct HttpWebFetcher {
    client: Option<reqwest::Client>,
}

#[cfg(feature = "websearch")]
impl HttpWebFetcher {
    pub fn new() -> Self {
        Self {
            client: crate::pipeline::http::build_client(),
        }
    }
}

#[cfg(feature = "websearch")]
impl Default for HttpWebFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "websearch")]
impl WebFetcher for HttpWebFetcher {
    fn search<'a>(
        &'a self,
        spec: &'static WebEngineSpec,
        query: &'a str,
        mailto: &'a str,
        max_hits: usize,
    ) -> BoxedHitsFuture<'a> {
        Box::pin(async move {
            match &self.client {
                Some(client) => fetch_hits(client, spec, query, mailto, max_hits)
                    .await
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        })
    }
}

#[cfg(feature = "websearch")]
async fn fetch_hits(
    client: &reqwest::Client,
    spec: &'static WebEngineSpec,
    query: &str,
    mailto: &str,
    max_hits: usize,
) -> Option<Vec<WebHit>> {
    use crate::core::websearch::{RequestShape, WebCategory, encoded_params, parse_hits};
    let encoded = encoded_params(spec, query, mailto);
    let request = match spec.shape {
        RequestShape::Get => client.get(format!("{}?{encoded}", spec.url)),
        RequestShape::PostForm => client
            .post(spec.url)
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

#[cfg(feature = "websearch")]
pub fn default_fetcher() -> Arc<dyn WebFetcher> {
    Arc::new(HttpWebFetcher::new())
}

#[cfg(not(feature = "websearch"))]
pub fn default_fetcher() -> Arc<dyn WebFetcher> {
    Arc::new(NoopWebFetcher)
}

pub async fn websearch_stage(
    fetcher: &dyn WebFetcher,
    plan: &SearchPlan,
    config: &WebSearchConfig,
    tx: &Sender<SearchEvent>,
) -> Vec<Vec<WebHit>> {
    let engines = if config.enabled {
        engines_for_mode(plan.mode, &config.engines)
    } else {
        Vec::new()
    };
    if engines.is_empty() {
        return vec![Vec::new(); plan.sub_queries.len()];
    }
    let query_futures: Vec<_> = plan
        .sub_queries
        .iter()
        .map(|sub| query_hits(fetcher, &engines, &sub.query, config))
        .collect();
    let per_query: Vec<Vec<WebHit>> = stream::iter(query_futures)
        .buffered(CONCURRENT_WEB_QUERIES)
        .collect()
        .await;
    let count = per_query.iter().map(Vec::len).sum();
    let _ = tx.send(SearchEvent::WebHits { count }).await;
    per_query
}

async fn query_hits(
    fetcher: &dyn WebFetcher,
    engines: &[&'static WebEngineSpec],
    query: &str,
    config: &WebSearchConfig,
) -> Vec<WebHit> {
    let deadline = tokio::time::Instant::now() + WEB_QUERY_TIMEOUT;
    let budget = usize::from(config.max_hits_per_query);
    let mut hits: Vec<WebHit> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for spec in engines {
        let remaining = budget.saturating_sub(hits.len());
        let time_left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining == 0 || time_left.is_zero() {
            break;
        }
        let engine_hits = tokio::time::timeout(
            time_left,
            engine_hits_with_fallback(fetcher, spec, query, &config.mailto, remaining),
        )
        .await
        .unwrap_or_default();
        hits.extend(
            engine_hits
                .into_iter()
                .filter(|hit| seen.insert(normalize_url(&hit.url)))
                .take(remaining),
        );
    }
    hits
}

async fn engine_hits_with_fallback(
    fetcher: &dyn WebFetcher,
    spec: &'static WebEngineSpec,
    query: &str,
    mailto: &str,
    max_hits: usize,
) -> Vec<WebHit> {
    let primary = fetcher.search(spec, query, mailto, max_hits).await;
    if primary.is_empty()
        && let Some(fallback) = fallback_engine(spec)
    {
        return fetcher.search(fallback, query, mailto, max_hits).await;
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
            _query: &'a str,
            _mailto: &'a str,
            max_hits: usize,
        ) -> BoxedHitsFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(spec.name);
                if self.slow_engines.contains(&spec.name) {
                    tokio::time::sleep(Duration::from_secs(600)).await;
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
        assert_eq!(events, vec![SearchEvent::WebHits { count: 4 }]);
    }

    #[tokio::test]
    async fn waterfall_stops_once_the_budget_is_met() {
        let fetcher = FakeFetcher::with(BTreeMap::from([(
            "ddg",
            vec![hit("https://a.example/1"), hit("https://a.example/2")],
        )]));
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(2)).await;
        assert_eq!(hits[0].len(), 2);
        assert_eq!(fetcher.called(), vec!["ddg"]);
    }

    #[tokio::test]
    async fn empty_primary_engine_falls_back_before_moving_on() {
        let fetcher = FakeFetcher::with(BTreeMap::from([(
            "ddg-lite",
            vec![hit("https://lite.example/1")],
        )]));
        let (hits, _) = run_stage(&fetcher, &plan(&["only"]), &config(1)).await;
        assert_eq!(hits[0][0].url, "https://lite.example/1");
        assert_eq!(fetcher.called(), vec!["ddg", "ddg-lite"]);
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
}
