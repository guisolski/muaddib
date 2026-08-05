use crate::core::citations::normalize_url;
use crate::core::config::WebSearchConfig;
use crate::core::plan::SearchPlan;
use crate::core::readability::{
    PageText, assign_pages, excerpt, extract_readable_text, page_targets,
};
use crate::core::websearch::{WebHit, page_grounding_enabled};
use crate::pipeline::SearchEvent;
use crate::pipeline::websearch::WebFetcher;
use futures::stream::{self, StreamExt};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub const PAGE_STAGE_DEADLINE: Duration = Duration::from_secs(10);
const CONCURRENT_PAGE_FETCHES: usize = 4;

pub async fn page_fetch_stage(
    fetcher: &dyn WebFetcher,
    plan: &SearchPlan,
    hits: &[Vec<WebHit>],
    config: &WebSearchConfig,
    tx: &Sender<SearchEvent>,
) -> Vec<Vec<PageText>> {
    if !page_grounding_enabled(plan.mode, config) {
        return vec![Vec::new(); hits.len()];
    }
    let top_n = usize::from(config.ground_top_n);
    let targets = page_targets(hits, top_n);
    if targets.is_empty() {
        return vec![Vec::new(); hits.len()];
    }
    let deadline = tokio::time::Instant::now() + PAGE_STAGE_DEADLINE;
    let max_chars = config.ground_page_chars as usize;
    let fetched: BTreeMap<String, String> = stream::iter(targets)
        .map(|url| fetch_page_text(fetcher, url, max_chars, deadline, tx))
        .buffer_unordered(CONCURRENT_PAGE_FETCHES)
        .filter_map(|entry| async move { entry })
        .collect()
        .await;
    assign_pages(hits, top_n, &fetched)
}

async fn fetch_page_text(
    fetcher: &dyn WebFetcher,
    url: String,
    max_chars: usize,
    deadline: tokio::time::Instant,
    tx: &Sender<SearchEvent>,
) -> Option<(String, String)> {
    let time_left = deadline.saturating_duration_since(tokio::time::Instant::now());
    let body = if time_left.is_zero() {
        None
    } else {
        tokio::time::timeout(time_left, fetcher.fetch_page(&url))
            .await
            .ok()
            .flatten()
    };
    let text = body
        .map(|html| excerpt(&extract_readable_text(&html), max_chars))
        .filter(|text| !text.is_empty());
    let ok = text.is_some();
    let _ = tx
        .send(SearchEvent::PageFetched {
            url: url.clone(),
            ok,
        })
        .await;
    text.map(|text| (normalize_url(&url), text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mode::Mode;
    use crate::core::plan::SubQuery;
    use crate::core::websearch::WebEngineSpec;
    use crate::pipeline::websearch::{BoxedHitsFuture, BoxedPageFuture};
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    struct FakePageFetcher {
        pages: BTreeMap<&'static str, &'static str>,
        slow_urls: Vec<&'static str>,
        calls: Mutex<Vec<String>>,
    }

    impl FakePageFetcher {
        fn with(pages: BTreeMap<&'static str, &'static str>) -> Self {
            Self {
                pages,
                slow_urls: Vec::new(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn called(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl WebFetcher for FakePageFetcher {
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

        fn fetch_page<'a>(&'a self, url: &'a str) -> BoxedPageFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(url.to_string());
                if self.slow_urls.contains(&url) {
                    tokio::time::sleep(Duration::from_secs(600)).await;
                }
                self.pages.get(url).map(|body| (*body).to_string())
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

    fn plan(mode: Mode, queries: &[&str]) -> SearchPlan {
        SearchPlan {
            original: "q".to_string(),
            mode,
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

    async fn run_stage(
        fetcher: &FakePageFetcher,
        plan: &SearchPlan,
        hits: &[Vec<WebHit>],
        config: &WebSearchConfig,
    ) -> (Vec<Vec<PageText>>, Vec<SearchEvent>) {
        let (tx, mut rx) = mpsc::channel(16);
        let pages = page_fetch_stage(fetcher, plan, hits, config, &tx).await;
        drop(tx);
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        (pages, events)
    }

    const BODY: &str =
        "<html><body><article><p>Study of desert rodents.</p></article></body></html>";

    #[tokio::test]
    async fn grounded_mode_fetches_top_pages_once_and_extracts_text() {
        let fetcher = FakePageFetcher::with(BTreeMap::from([
            ("https://a.example", BODY),
            ("https://b.example", BODY),
        ]));
        let hits = vec![
            vec![hit("https://a.example"), hit("https://b.example")],
            vec![hit("https://a.example")],
        ];
        let config = WebSearchConfig::default();
        let (pages, events) = run_stage(
            &fetcher,
            &plan(Mode::Scientific, &["one", "two"]),
            &hits,
            &config,
        )
        .await;
        assert_eq!(pages[0].len(), 2);
        assert_eq!(pages[1].len(), 1);
        assert_eq!(pages[0][0].text, "Study of desert rodents.");
        assert_eq!(fetcher.called().len(), 2);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, SearchEvent::PageFetched { ok, .. } if *ok))
        );
    }

    #[tokio::test]
    async fn ungrounded_mode_skips_fetching_entirely() {
        let fetcher = FakePageFetcher::with(BTreeMap::from([("https://a.example", BODY)]));
        let hits = vec![vec![hit("https://a.example")]];
        let config = WebSearchConfig::default();
        let (pages, events) =
            run_stage(&fetcher, &plan(Mode::General, &["one"]), &hits, &config).await;
        assert_eq!(pages, vec![Vec::new()]);
        assert!(events.is_empty());
        assert!(fetcher.called().is_empty());
    }

    #[tokio::test]
    async fn failed_and_empty_pages_degrade_silently() {
        let fetcher = FakePageFetcher::with(BTreeMap::from([(
            "https://empty.example",
            "<html><body><script>x</script></body></html>",
        )]));
        let hits = vec![vec![
            hit("https://empty.example"),
            hit("https://gone.example"),
        ]];
        let config = WebSearchConfig::default();
        let (pages, events) =
            run_stage(&fetcher, &plan(Mode::Deep, &["one"]), &hits, &config).await;
        assert_eq!(pages, vec![Vec::new()]);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, SearchEvent::PageFetched { ok, .. } if !*ok))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_pages_time_out_keeping_the_fast_ones() {
        let fetcher = FakePageFetcher {
            pages: BTreeMap::from([
                ("https://fast.example", BODY),
                ("https://slow.example", BODY),
            ]),
            slow_urls: vec!["https://slow.example"],
            calls: Mutex::new(Vec::new()),
        };
        let hits = vec![vec![
            hit("https://fast.example"),
            hit("https://slow.example"),
        ]];
        let config = WebSearchConfig::default();
        let (pages, _) = run_stage(&fetcher, &plan(Mode::Deep, &["one"]), &hits, &config).await;
        assert_eq!(pages[0].len(), 1);
        assert_eq!(pages[0][0].url, "https://fast.example");
    }

    #[tokio::test]
    async fn page_excerpts_respect_the_char_cap() {
        let long_body = format!(
            "<html><body><article><p>{}</p></article></body></html>",
            "word ".repeat(300)
        );
        let leaked: &'static str = Box::leak(long_body.into_boxed_str());
        let fetcher = FakePageFetcher::with(BTreeMap::from([("https://a.example", leaked)]));
        let hits = vec![vec![hit("https://a.example")]];
        let config = WebSearchConfig {
            ground_page_chars: 500,
            ..WebSearchConfig::default()
        };
        let (pages, _) =
            run_stage(&fetcher, &plan(Mode::Scientific, &["one"]), &hits, &config).await;
        assert_eq!(pages[0][0].text.chars().count(), 501);
        assert!(pages[0][0].text.ends_with('\u{2026}'));
    }
}
