use crate::core::answer::{ANSWER_SCHEMA, Answer, FAST_ANSWER_SCHEMA, parse_answer};
use crate::core::citations::{
    MergedFindings, SubResult, allowed_image_urls, eject_unknown_images, merge_sub_results,
    parse_sub_response, renumber_sources, self_declared_urls, strip_image_blocks,
};
use crate::core::config::{Config, WebSearchConfig};
use crate::core::context::{ResearchContext, context_allowed_urls};
use crate::core::extract::extract_json;
use crate::core::mode::{Mode, ModeSpec};
use crate::core::plan::{
    SearchPlan, SubQuery, effective_breadth, literal_plan, plan_from_expansion, synthesis_timeout,
};
use crate::core::prompts::{expansion_prompt, fast_prompt, sub_search_prompt, synthesis_prompt};
use crate::core::readability::PageText;
use crate::core::websearch::{WebHit, allowed_urls_with_hits, snippet_sub_results};
use crate::engines::{Engine, EngineError, EngineJob, EngineOutput};
use crate::pipeline::websearch::{WebFetcher, default_fetcher, websearch_stage};
use crate::pipeline::{SearchEvent, SearchHandle};
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub const EXPANSION_TIMEOUT: Duration = Duration::from_secs(45);
pub const FAST_TARGET_SECS: u64 = 5;
const EVENT_BUFFER: usize = 64;

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub mode: Mode,
    pub answer_lang: String,
    pub breadth: u8,
    pub max_parallel: usize,
    pub engine_timeout: Duration,
    pub fast: bool,
    pub fast_timeout: Duration,
    pub validate_links: bool,
    pub fetch_images: bool,
    pub websearch: WebSearchConfig,
    pub context: ResearchContext,
}

impl SearchRequest {
    pub fn from_config(query: String, mode: Mode, fast: bool, config: &Config) -> Self {
        Self {
            query,
            mode,
            answer_lang: config.language.clone(),
            breadth: effective_breadth(mode, config.expansion_breadth),
            max_parallel: usize::from(config.max_parallel),
            engine_timeout: Duration::from_secs(config.engine_timeout_secs),
            fast,
            fast_timeout: Duration::from_secs(config.fast_timeout_secs),
            validate_links: config.validate_links,
            fetch_images: config.images && !fast,
            websearch: if fast {
                WebSearchConfig::disabled()
            } else {
                config.websearch.clone()
            },
            context: ResearchContext::default(),
        }
    }
}

pub fn spawn_search(engine: Arc<dyn Engine>, request: SearchRequest) -> SearchHandle {
    spawn_search_with_fetcher(engine, default_fetcher(), request)
}

pub fn spawn_search_with_fetcher(
    engine: Arc<dyn Engine>,
    fetcher: Arc<dyn WebFetcher>,
    request: SearchRequest,
) -> SearchHandle {
    let (tx, events) = mpsc::channel(EVENT_BUFFER);
    let task = tokio::spawn(run_search(engine, fetcher, request, tx));
    SearchHandle::new(events, task)
}

pub async fn run_search(
    engine: Arc<dyn Engine>,
    fetcher: Arc<dyn WebFetcher>,
    request: SearchRequest,
    tx: mpsc::Sender<SearchEvent>,
) {
    match run_stages(engine.as_ref(), fetcher.as_ref(), &request, &tx).await {
        Ok(()) => send(&tx, SearchEvent::Completed).await,
        Err(message) => send(&tx, SearchEvent::Failed(message)).await,
    }
}

async fn send(tx: &mpsc::Sender<SearchEvent>, event: SearchEvent) {
    let _ = tx.send(event).await;
}

async fn run_stages(
    engine: &dyn Engine,
    fetcher: &dyn WebFetcher,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<(), String> {
    if request.fast {
        return run_fast_stages(engine, request, tx).await;
    }
    let plan = expansion_stage(engine, request, tx).await;
    send(tx, SearchEvent::PlanReady(plan.clone())).await;
    let hits = websearch_stage(fetcher, &plan, &request.websearch, tx).await;
    let pages = page_grounding_stage(fetcher, &plan, &hits, &request.websearch, tx).await;
    let sub_results = fanout_stage(engine, &plan, &hits, &pages, request, tx).await;
    if sub_results.is_empty() {
        return Err("every sub-query failed; nothing to synthesize".to_string());
    }
    let merged = merged_findings(&sub_results, &plan, &hits, request);
    if merged.findings.is_empty() {
        return Err("the searches produced no findings with usable sources".to_string());
    }
    send(tx, SearchEvent::SynthesisStarted).await;
    let all_hits: Vec<WebHit> = hits.into_iter().flatten().collect();
    let answer = synthesis_stage(engine, &plan, &merged, &all_hits, request, tx).await?;
    send(tx, SearchEvent::AnswerReady(Box::new(answer.clone()))).await;
    link_validation_stage(&answer, request, tx).await;
    image_fetch_stage(&answer, request, tx).await;
    Ok(())
}

fn merged_findings(
    sub_results: &[SubResult],
    plan: &SearchPlan,
    hits: &[Vec<WebHit>],
    request: &SearchRequest,
) -> MergedFindings {
    if request.websearch.merge_snippets {
        let combined: Vec<SubResult> = sub_results
            .iter()
            .cloned()
            .chain(snippet_sub_results(&plan.sub_queries, hits))
            .collect();
        merge_sub_results(&combined)
    } else {
        merge_sub_results(sub_results)
    }
}

async fn run_fast_stages(
    engine: &dyn Engine,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<(), String> {
    let plan = literal_plan(&request.query, request.mode, &request.answer_lang);
    send(tx, SearchEvent::PlanReady(plan)).await;
    send(tx, SearchEvent::SubQueryStarted { idx: 0 }).await;
    let result = fast_answer_stage(engine, request, tx).await;
    send(
        tx,
        SearchEvent::SubQueryFinished {
            idx: 0,
            ok: result.is_ok(),
        },
    )
    .await;
    let answer = result?;
    send(tx, SearchEvent::AnswerReady(Box::new(answer.clone()))).await;
    link_validation_stage(&answer, request, tx).await;
    Ok(())
}

async fn fast_answer_stage(
    engine: &dyn Engine,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<Answer, String> {
    let inline_schema = !engine.supports_json_schema();
    let job = EngineJob {
        prompt: fast_prompt(
            &request.query,
            request.mode.spec(),
            &request.answer_lang,
            inline_schema,
            &request.context,
        ),
        schema: Some(FAST_ANSWER_SCHEMA),
        timeout: request.fast_timeout,
    };
    let output = run_reporting_usage(engine, &job, tx)
        .await
        .map_err(|error| format!("fast search failed: {error}"))?;
    let value = extract_json(&output.text)
        .ok_or_else(|| "fast search returned no parsable JSON".to_string())?;
    let answer = parse_answer(value)
        .map_err(|error| format!("fast search JSON did not match the answer schema: {error}"))?;
    let allowed = self_declared_urls(&answer);
    Ok(renumber_sources(strip_image_blocks(answer), &allowed))
}

async fn run_reporting_usage(
    engine: &dyn Engine,
    job: &EngineJob,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<EngineOutput, EngineError> {
    let result = engine.run(job).await;
    if let Ok(output) = &result
        && let Some(usage) = output.usage
    {
        send(tx, SearchEvent::CallCosted { usage }).await;
    }
    result
}

async fn expansion_stage(
    engine: &dyn Engine,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> SearchPlan {
    let prompt = expansion_prompt(
        &request.query,
        request.mode.spec(),
        &request.answer_lang,
        request.breadth,
        &request.context,
    );
    let job = EngineJob {
        prompt,
        schema: None,
        timeout: EXPANSION_TIMEOUT,
    };
    let expansion = match run_reporting_usage(engine, &job, tx).await {
        Ok(output) => extract_json(&output.text).unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::Null,
    };
    plan_from_expansion(
        &request.query,
        request.mode,
        &request.answer_lang,
        &expansion,
        request.breadth,
    )
}

struct GroundedSubQuery<'a> {
    sub: &'a SubQuery,
    hits: &'a [WebHit],
    pages: &'a [PageText],
}

async fn fanout_stage(
    engine: &dyn Engine,
    plan: &SearchPlan,
    hits: &[Vec<WebHit>],
    pages: &[Vec<PageText>],
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Vec<SubResult> {
    let mode_spec = plan.mode.spec();
    let sub_query_futures: Vec<_> = plan
        .sub_queries
        .iter()
        .enumerate()
        .map(|(idx, sub)| {
            let grounded = GroundedSubQuery {
                sub,
                hits: hits.get(idx).map_or(&[][..], Vec::as_slice),
                pages: pages.get(idx).map_or(&[][..], Vec::as_slice),
            };
            track_sub_query(engine, idx, grounded, mode_spec, request, tx)
        })
        .collect();
    stream::iter(sub_query_futures)
        .buffer_unordered(request.max_parallel.max(1))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect()
}

async fn track_sub_query(
    engine: &dyn Engine,
    idx: usize,
    grounded: GroundedSubQuery<'_>,
    mode_spec: &ModeSpec,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Option<SubResult> {
    send(tx, SearchEvent::SubQueryStarted { idx }).await;
    let result = run_sub_query(engine, &grounded, mode_spec, request.engine_timeout, tx).await;
    let ok = result.is_some();
    send(tx, SearchEvent::SubQueryFinished { idx, ok }).await;
    result
}

async fn run_sub_query(
    engine: &dyn Engine,
    grounded: &GroundedSubQuery<'_>,
    mode_spec: &ModeSpec,
    timeout: Duration,
    tx: &mpsc::Sender<SearchEvent>,
) -> Option<SubResult> {
    let job = EngineJob {
        prompt: sub_search_prompt(grounded.sub, mode_spec, grounded.hits, grounded.pages),
        schema: None,
        timeout,
    };
    let output = run_reporting_usage(engine, &job, tx).await.ok()?;
    let value = extract_json(&output.text)?;
    let response = parse_sub_response(&value)?;
    Some(SubResult {
        query: grounded.sub.query.clone(),
        lang: grounded.sub.lang.clone(),
        response,
    })
}

async fn synthesis_stage(
    engine: &dyn Engine,
    plan: &SearchPlan,
    merged: &MergedFindings,
    hits: &[WebHit],
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<Answer, String> {
    let inline_schema = !engine.supports_json_schema();
    let job = EngineJob {
        prompt: synthesis_prompt(plan, merged, inline_schema, &request.context),
        schema: Some(ANSWER_SCHEMA),
        timeout: synthesis_timeout(request.engine_timeout, plan.sub_queries.len()),
    };
    let output = run_reporting_usage(engine, &job, tx)
        .await
        .map_err(|error| format!("synthesis failed: {error}"))?;
    let value = extract_json(&output.text)
        .ok_or_else(|| "synthesis returned no parsable JSON".to_string())?;
    let answer = parse_answer(value)
        .map_err(|error| format!("synthesis JSON did not match the answer schema: {error}"))?;
    let answer = eject_unknown_images(answer, &allowed_image_urls(merged));
    let allowed: std::collections::BTreeSet<String> = allowed_urls_with_hits(merged, hits)
        .into_iter()
        .chain(context_allowed_urls(&request.context))
        .collect();
    Ok(renumber_sources(answer, &allowed))
}

#[cfg(feature = "websearch")]
async fn page_grounding_stage(
    fetcher: &dyn WebFetcher,
    plan: &SearchPlan,
    hits: &[Vec<WebHit>],
    config: &WebSearchConfig,
    tx: &mpsc::Sender<SearchEvent>,
) -> Vec<Vec<PageText>> {
    crate::pipeline::pages::page_fetch_stage(fetcher, plan, hits, config, tx).await
}

#[cfg(not(feature = "websearch"))]
async fn page_grounding_stage(
    _fetcher: &dyn WebFetcher,
    _plan: &SearchPlan,
    hits: &[Vec<WebHit>],
    _config: &WebSearchConfig,
    _tx: &mpsc::Sender<SearchEvent>,
) -> Vec<Vec<PageText>> {
    vec![Vec::new(); hits.len()]
}

#[cfg(feature = "link-validation")]
async fn link_validation_stage(
    answer: &Answer,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) {
    if request.validate_links {
        crate::pipeline::validate::validate_links(&answer.sources, tx).await;
    }
}

#[cfg(not(feature = "link-validation"))]
async fn link_validation_stage(
    _answer: &Answer,
    _request: &SearchRequest,
    _tx: &mpsc::Sender<SearchEvent>,
) {
}

#[cfg(feature = "link-validation")]
async fn image_fetch_stage(
    answer: &Answer,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) {
    if request.fetch_images {
        crate::pipeline::images::fetch_images(answer, tx).await;
    }
}

#[cfg(not(feature = "link-validation"))]
async fn image_fetch_stage(
    _answer: &Answer,
    _request: &SearchRequest,
    _tx: &mpsc::Sender<SearchEvent>,
) {
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::prompts::{
        EXPANSION_MARKER, FAST_MARKER, SUB_SEARCH_MARKER, SYNTHESIS_MARKER,
    };
    use crate::core::websearch::WebHit;
    use crate::engines::{BoxedEngineFuture, EngineError, EngineOutput};
    use crate::pipeline::websearch::{BoxedHitsFuture, NoopWebFetcher};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeEngine {
        fail_markers: Vec<&'static str>,
        simple_expansion: bool,
        calls: AtomicUsize,
        prompts: Mutex<Vec<String>>,
    }

    impl FakeEngine {
        fn reliable() -> Self {
            Self {
                fail_markers: vec![],
                simple_expansion: false,
                calls: AtomicUsize::new(0),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn failing_on(markers: &[&'static str]) -> Self {
            Self {
                fail_markers: markers.to_vec(),
                ..Self::reliable()
            }
        }

        fn rating_simple() -> Self {
            Self {
                simple_expansion: true,
                ..Self::reliable()
            }
        }

        fn prompts_with(&self, marker: &str) -> Vec<String> {
            self.prompts
                .lock()
                .unwrap()
                .iter()
                .filter(|prompt| prompt.contains(marker))
                .cloned()
                .collect()
        }

        fn canned_response(&self, marker: &str) -> String {
            match marker {
                EXPANSION_MARKER if self.simple_expansion => r#"{"complexity":"simple","subqueries":[
                    {"query":"rust async runtimes","lang":"en","rationale":"literal"},
                    {"query":"rust async runtimes comparison","lang":"en","rationale":"facet"}
                ]}"#
                .to_string(),
                EXPANSION_MARKER => r#"{"subqueries":[
                    {"query":"topic overview","lang":"en","rationale":"facet"},
                    {"query":"tema em detalhe","lang":"pt-BR","rationale":"facet"}
                ]}"#
                    .to_string(),
                SUB_SEARCH_MARKER => r#"{"summary":"found things","findings":[
                    {"claim":"claim one","source_title":"One","source_url":"https://one.example/a","lang":"en","image_url":"https://one.example/figure.png"},
                    {"claim":"claim two","source_title":"Two","source_url":"https://two.example/b","lang":"en"}
                ]}"#
                    .to_string(),
                SYNTHESIS_MARKER => r#"{
                    "title":"Compiled",
                    "language":"en",
                    "blocks":[
                        {"type":"paragraph","text":"real claim","source_ids":[1,2]},
                        {"type":"paragraph","text":"hallucinated claim","source_ids":[3]},
                        {"type":"image","url":"https://one.example/figure.png","caption":"real figure","source_ids":[1]},
                        {"type":"image","url":"https://invented.example/fake.png","caption":"invented figure","source_ids":[2]}
                    ],
                    "sources":[
                        {"id":1,"title":"One","url":"https://one.example/a","lang":"en"},
                        {"id":2,"title":"Two","url":"https://two.example/b","lang":"en"},
                        {"id":3,"title":"Fake","url":"https://invented.example/x","lang":"en"}
                    ],
                    "followups":["next question"]
                }"#
                .to_string(),
                FAST_MARKER => r#"{
                    "title":"Quick answer",
                    "language":"en",
                    "blocks":[
                        {"type":"paragraph","text":"the short answer","source_ids":[1]},
                        {"type":"image","url":"https://one.example/figure.png","source_ids":[1]},
                        {"type":"paragraph","text":"unsupported claim","source_ids":[9]}
                    ],
                    "sources":[
                        {"id":1,"title":"One","url":"https://one.example/a","lang":"en"},
                        {"id":9,"title":"Bogus","url":"not-a-url","lang":"en"}
                    ],
                    "followups":["next question"]
                }"#
                .to_string(),
                _ => panic!("prompt carries no known marker"),
            }
        }
    }

    fn marker_of(prompt: &str) -> &'static str {
        [
            EXPANSION_MARKER,
            SUB_SEARCH_MARKER,
            SYNTHESIS_MARKER,
            FAST_MARKER,
        ]
        .into_iter()
        .find(|marker| prompt.contains(marker))
        .expect("prompt carries a routing marker")
    }

    impl Engine for FakeEngine {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn run<'a>(&'a self, job: &'a EngineJob) -> BoxedEngineFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.prompts.lock().unwrap().push(job.prompt.clone());
                let marker = marker_of(&job.prompt);
                if self.fail_markers.contains(&marker) {
                    return Err(EngineError::Reported(format!("forced failure: {marker}")));
                }
                Ok(EngineOutput::from_text(self.canned_response(marker)))
            })
        }
    }

    struct FakeWebFetcher {
        canned: Vec<WebHit>,
        page_body: Option<&'static str>,
        calls: AtomicUsize,
        page_calls: AtomicUsize,
    }

    impl FakeWebFetcher {
        fn returning(canned: Vec<WebHit>) -> Self {
            Self {
                canned,
                page_body: None,
                calls: AtomicUsize::new(0),
                page_calls: AtomicUsize::new(0),
            }
        }

        fn with_pages(canned: Vec<WebHit>, page_body: &'static str) -> Self {
            Self {
                page_body: Some(page_body),
                ..Self::returning(canned)
            }
        }
    }

    impl crate::pipeline::websearch::WebFetcher for FakeWebFetcher {
        fn search<'a>(
            &'a self,
            _spec: &'static crate::core::websearch::WebEngineSpec,
            _base_url: &'a str,
            _query: &'a str,
            _mailto: &'a str,
            max_hits: usize,
        ) -> BoxedHitsFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.canned.iter().take(max_hits).cloned().collect()
            })
        }

        fn fetch_page<'a>(
            &'a self,
            _url: &'a str,
        ) -> crate::pipeline::websearch::BoxedPageFuture<'a> {
            Box::pin(async move {
                self.page_calls.fetch_add(1, Ordering::SeqCst);
                self.page_body.map(str::to_string)
            })
        }
    }

    fn web_hit(url: &str, snippet: &str) -> WebHit {
        WebHit {
            title: "Hit title".to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
            engine: "ddg",
        }
    }

    fn request() -> SearchRequest {
        SearchRequest {
            query: "rust async runtimes".to_string(),
            mode: Mode::General,
            answer_lang: "en".to_string(),
            breadth: 3,
            max_parallel: 4,
            engine_timeout: Duration::from_secs(5),
            fast: false,
            fast_timeout: Duration::from_secs(5),
            validate_links: false,
            fetch_images: false,
            websearch: WebSearchConfig::disabled(),
            context: ResearchContext::default(),
        }
    }

    fn fast_request() -> SearchRequest {
        SearchRequest {
            fast: true,
            ..request()
        }
    }

    fn websearch_request(merge_snippets: bool) -> SearchRequest {
        SearchRequest {
            websearch: WebSearchConfig {
                merge_snippets,
                ..WebSearchConfig::default()
            },
            ..request()
        }
    }

    async fn collect_events(engine: FakeEngine) -> Vec<SearchEvent> {
        drain(engine, request()).await.1
    }

    async fn drain(engine: FakeEngine, request: SearchRequest) -> (usize, Vec<SearchEvent>) {
        let engine = Arc::new(engine);
        let events = drain_with(engine.clone(), Arc::new(NoopWebFetcher), request).await;
        (engine.calls.load(Ordering::SeqCst), events)
    }

    async fn drain_with(
        engine: Arc<FakeEngine>,
        fetcher: Arc<dyn crate::pipeline::websearch::WebFetcher>,
        request: SearchRequest,
    ) -> Vec<SearchEvent> {
        let mut handle = spawn_search_with_fetcher(engine, fetcher, request);
        let mut events = Vec::new();
        while let Some(event) = handle.events.recv().await {
            events.push(event);
        }
        events
    }

    fn answer_in(events: &[SearchEvent]) -> &Answer {
        events
            .iter()
            .find_map(|event| match event {
                SearchEvent::AnswerReady(answer) => Some(answer.as_ref()),
                _ => None,
            })
            .expect("answer produced")
    }

    #[tokio::test]
    async fn happy_path_emits_the_full_event_sequence() {
        let events = collect_events(FakeEngine::reliable()).await;
        assert!(matches!(events.first(), Some(SearchEvent::PlanReady(_))));
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        let started = events
            .iter()
            .filter(|event| matches!(event, SearchEvent::SubQueryStarted { .. }))
            .count();
        let finished = events
            .iter()
            .filter(|event| matches!(event, SearchEvent::SubQueryFinished { ok: true, .. }))
            .count();
        assert_eq!(started, 3);
        assert_eq!(finished, 3);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SearchEvent::SynthesisStarted))
        );
    }

    #[tokio::test]
    async fn plan_includes_original_query_plus_expansion() {
        let events = collect_events(FakeEngine::reliable()).await;
        let Some(SearchEvent::PlanReady(plan)) = events.first() else {
            panic!("expected PlanReady first");
        };
        assert_eq!(plan.sub_queries.len(), 3);
        assert_eq!(plan.sub_queries[0].query, "rust async runtimes");
        assert_eq!(plan.sub_queries[2].lang, "pt-BR");
    }

    #[tokio::test]
    async fn answer_is_renumbered_and_hallucinated_sources_are_ejected() {
        let events = collect_events(FakeEngine::reliable()).await;
        let answer = events
            .iter()
            .find_map(|event| match event {
                SearchEvent::AnswerReady(answer) => Some(answer),
                _ => None,
            })
            .expect("answer produced");
        let urls: Vec<&str> = answer
            .sources
            .iter()
            .map(|source| source.url.as_str())
            .collect();
        assert_eq!(urls, vec!["https://one.example/a", "https://two.example/b"]);
        assert!(answer.blocks.iter().all(|block| match block {
            crate::core::answer::Block::Paragraph { source_ids, .. } =>
                source_ids.iter().all(|id| *id <= 2),
            _ => true,
        }));
    }

    #[tokio::test]
    async fn hallucinated_image_blocks_are_ejected() {
        let events = collect_events(FakeEngine::reliable()).await;
        let answer = events
            .iter()
            .find_map(|event| match event {
                SearchEvent::AnswerReady(answer) => Some(answer),
                _ => None,
            })
            .expect("answer produced");
        let image_urls: Vec<&str> = answer
            .blocks
            .iter()
            .filter_map(|block| match block {
                crate::core::answer::Block::Image { url, .. } => Some(url.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(image_urls, vec!["https://one.example/figure.png"]);
    }

    #[tokio::test]
    async fn simple_complexity_runs_a_single_sub_search() {
        let events = collect_events(FakeEngine::rating_simple()).await;
        let started = events
            .iter()
            .filter(|event| matches!(event, SearchEvent::SubQueryStarted { .. }))
            .count();
        assert_eq!(started, 1);
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
    }

    #[tokio::test]
    async fn expansion_failure_degrades_to_fallback_plan() {
        let events = collect_events(FakeEngine::failing_on(&[EXPANSION_MARKER])).await;
        let Some(SearchEvent::PlanReady(plan)) = events.first() else {
            panic!("expected PlanReady first");
        };
        assert_eq!(plan.sub_queries.len(), 3);
        assert_eq!(plan.sub_queries[0].query, "rust async runtimes");
        assert_eq!(plan.sub_queries[0].rationale, "literal query");
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
    }

    #[tokio::test]
    async fn total_sub_query_failure_reports_a_search_failure() {
        let events = collect_events(FakeEngine::failing_on(&[SUB_SEARCH_MARKER])).await;
        assert!(matches!(events.last(), Some(SearchEvent::Failed(_))));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SearchEvent::SynthesisStarted))
        );
    }

    #[tokio::test]
    async fn synthesis_failure_reports_a_search_failure() {
        let events = collect_events(FakeEngine::failing_on(&[SYNTHESIS_MARKER])).await;
        let Some(SearchEvent::Failed(message)) = events.last() else {
            panic!("expected Failed last");
        };
        assert!(message.contains("synthesis failed"));
    }

    #[tokio::test]
    async fn fast_mode_answers_with_a_single_engine_call() {
        let (calls, events) = drain(FakeEngine::reliable(), fast_request()).await;
        assert_eq!(calls, 1);
        assert!(matches!(events.first(), Some(SearchEvent::PlanReady(_))));
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SearchEvent::SynthesisStarted))
        );
        let started = events
            .iter()
            .filter(|event| matches!(event, SearchEvent::SubQueryStarted { .. }))
            .count();
        assert_eq!(started, 1);
    }

    #[tokio::test]
    async fn fast_mode_plans_the_literal_query_without_expanding() {
        let (_, events) = drain(FakeEngine::reliable(), fast_request()).await;
        let Some(SearchEvent::PlanReady(plan)) = events.first() else {
            panic!("expected PlanReady first");
        };
        assert_eq!(plan.sub_queries.len(), 1);
        assert_eq!(plan.sub_queries[0].query, "rust async runtimes");
        assert_eq!(plan.sub_queries[0].rationale, "literal query");
    }

    #[tokio::test]
    async fn fast_mode_drops_images_and_sources_it_cannot_stand_behind() {
        let (_, events) = drain(FakeEngine::reliable(), fast_request()).await;
        let answer = answer_in(&events);
        assert!(
            !answer
                .blocks
                .iter()
                .any(|block| matches!(block, crate::core::answer::Block::Image { .. }))
        );
        let urls: Vec<&str> = answer
            .sources
            .iter()
            .map(|source| source.url.as_str())
            .collect();
        assert_eq!(urls, vec!["https://one.example/a"]);
    }

    #[tokio::test]
    async fn fast_mode_failure_reports_a_search_failure() {
        let (_, events) = drain(FakeEngine::failing_on(&[FAST_MARKER]), fast_request()).await;
        let Some(SearchEvent::Failed(message)) = events.last() else {
            panic!("expected Failed last");
        };
        assert!(message.contains("fast search failed"));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SearchEvent::SubQueryFinished { ok: false, .. }))
        );
    }

    #[tokio::test]
    async fn standard_mode_still_makes_three_engine_calls_for_a_simple_rating() {
        let (calls, _) = drain(FakeEngine::rating_simple(), request()).await;
        assert_eq!(calls, 3);
    }

    #[tokio::test]
    async fn web_hits_ground_every_sub_search_prompt() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::returning(vec![web_hit(
            "https://hit.example/page",
            "A grounding snippet.",
        )]));
        let events = drain_with(engine.clone(), fetcher, websearch_request(false)).await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SearchEvent::WebHits { count: 3, .. }))
        );
        let sub_prompts = engine.prompts_with(SUB_SEARCH_MARKER);
        assert_eq!(sub_prompts.len(), 3);
        for prompt in &sub_prompts {
            assert!(prompt.contains("Candidate sources"));
            assert!(prompt.contains("https://hit.example/page"));
        }
    }

    #[tokio::test]
    async fn hit_urls_extend_the_synthesis_allowlist() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::returning(vec![web_hit(
            "https://invented.example/x",
            "Actually a real search result.",
        )]));
        let events = drain_with(engine, fetcher, websearch_request(false)).await;
        let urls: Vec<&str> = answer_in(&events)
            .sources
            .iter()
            .map(|source| source.url.as_str())
            .collect();
        assert!(urls.contains(&"https://invented.example/x"));
    }

    #[tokio::test]
    async fn merge_snippets_feeds_hits_into_the_synthesis_findings() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::returning(vec![web_hit(
            "https://hit.example/page",
            "A snippet worth citing.",
        )]));
        drain_with(engine.clone(), fetcher, websearch_request(true)).await;
        let synthesis_prompts = engine.prompts_with(SYNTHESIS_MARKER);
        assert_eq!(synthesis_prompts.len(), 1);
        assert!(synthesis_prompts[0].contains("https://hit.example/page"));
        assert!(synthesis_prompts[0].contains("A snippet worth citing."));
    }

    #[tokio::test]
    async fn follow_up_context_reaches_prompts_and_allows_ancestor_sources() {
        use crate::core::context::ContextStep;
        let engine = Arc::new(FakeEngine::reliable());
        let context = ResearchContext {
            steps: vec![ContextStep {
                query: "earlier question".to_string(),
                summary: "earlier answer digest".to_string(),
                source_urls: vec!["https://invented.example/x".to_string()],
            }],
            omitted: 0,
        };
        let events = drain_with(
            engine.clone(),
            Arc::new(NoopWebFetcher),
            SearchRequest {
                context,
                ..request()
            },
        )
        .await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        for marker in [EXPANSION_MARKER, SYNTHESIS_MARKER] {
            let prompts = engine.prompts_with(marker);
            assert!(!prompts.is_empty(), "{marker}");
            assert!(prompts[0].contains("research thread"), "{marker}");
            assert!(prompts[0].contains("earlier question"), "{marker}");
        }
        let urls: Vec<&str> = answer_in(&events)
            .sources
            .iter()
            .map(|source| source.url.as_str())
            .collect();
        assert!(urls.contains(&"https://invented.example/x"));
    }

    #[tokio::test]
    async fn scientific_mode_grounds_sub_search_prompts_with_page_content() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::with_pages(
            vec![web_hit("https://hit.example/page", "snippet")],
            "<html><body><article><p>Peer-reviewed rodent study.</p></article></body></html>",
        ));
        let events = drain_with(
            engine.clone(),
            fetcher,
            SearchRequest {
                mode: Mode::Scientific,
                websearch: WebSearchConfig::default(),
                ..request()
            },
        )
        .await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, SearchEvent::PageFetched { ok, .. } if *ok))
        );
        let sub_prompts = engine.prompts_with(SUB_SEARCH_MARKER);
        assert!(!sub_prompts.is_empty());
        for prompt in sub_prompts {
            assert!(prompt.contains("Fetched page content"));
            assert!(prompt.contains("Peer-reviewed rodent study."));
        }
    }

    #[tokio::test]
    async fn general_mode_skips_page_grounding_by_default() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::with_pages(
            vec![web_hit("https://hit.example/page", "snippet")],
            "<html><body><p>ignored</p></body></html>",
        ));
        let events = drain_with(engine.clone(), fetcher.clone(), websearch_request(false)).await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        assert_eq!(fetcher.page_calls.load(Ordering::SeqCst), 0);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SearchEvent::PageFetched { .. }))
        );
        for prompt in engine.prompts_with(SUB_SEARCH_MARKER) {
            assert!(!prompt.contains("Fetched page content"));
        }
    }

    #[tokio::test]
    async fn empty_web_hits_degrade_to_the_ungrounded_flow() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::returning(Vec::new()));
        let events = drain_with(engine.clone(), fetcher, websearch_request(false)).await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        let urls: Vec<&str> = answer_in(&events)
            .sources
            .iter()
            .map(|source| source.url.as_str())
            .collect();
        assert_eq!(urls, vec!["https://one.example/a", "https://two.example/b"]);
        for prompt in engine.prompts_with(SUB_SEARCH_MARKER) {
            assert!(!prompt.contains("Candidate sources"));
        }
    }

    #[tokio::test]
    async fn disabled_websearch_never_calls_the_fetcher() {
        let engine = Arc::new(FakeEngine::reliable());
        let fetcher = Arc::new(FakeWebFetcher::returning(vec![web_hit(
            "https://hit.example/page",
            "snippet",
        )]));
        let events = drain_with(engine, fetcher.clone(), request()).await;
        assert!(matches!(events.last(), Some(SearchEvent::Completed)));
        assert_eq!(fetcher.calls.load(Ordering::SeqCst), 0);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, SearchEvent::WebHits { .. }))
        );
    }

    #[test]
    fn fast_request_from_config_disables_websearch() {
        let config = Config::default();
        let fast = SearchRequest::from_config("q".to_string(), Mode::General, true, &config);
        let standard = SearchRequest::from_config("q".to_string(), Mode::General, false, &config);
        assert!(!fast.websearch.enabled);
        assert!(standard.websearch.enabled);
    }

    #[test]
    fn request_from_config_applies_language_breadth_and_limits() {
        let config = Config {
            language: "fr".to_string(),
            max_parallel: 2,
            expansion_breadth: 0,
            engine_timeout_secs: 30,
            validate_links: false,
            ..Config::default()
        };
        let request = SearchRequest::from_config("q".to_string(), Mode::Deep, false, &config);
        assert_eq!(request.answer_lang, "fr");
        assert_eq!(request.breadth, 6);
        assert_eq!(request.max_parallel, 2);
        assert_eq!(request.engine_timeout, Duration::from_secs(30));
        assert!(!request.validate_links);
        assert!(request.fetch_images);
        assert!(!request.fast);
    }

    #[test]
    fn fast_request_carries_the_fast_timeout_and_disables_images() {
        let config = Config {
            fast_timeout_secs: 12,
            images: true,
            ..Config::default()
        };
        let request = SearchRequest::from_config("q".to_string(), Mode::General, true, &config);
        assert!(request.fast);
        assert_eq!(request.fast_timeout, Duration::from_secs(12));
        assert!(!request.fetch_images);
    }
}
