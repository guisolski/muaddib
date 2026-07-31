use crate::core::answer::{ANSWER_SCHEMA, Answer, FAST_ANSWER_SCHEMA, parse_answer};
use crate::core::citations::{
    MergedFindings, SubResult, allowed_image_urls, allowed_urls, eject_unknown_images,
    merge_sub_results, parse_sub_response, renumber_sources, self_declared_urls,
    strip_image_blocks,
};
use crate::core::config::Config;
use crate::core::extract::extract_json;
use crate::core::mode::{Mode, ModeSpec};
use crate::core::plan::{
    SearchPlan, SubQuery, effective_breadth, literal_plan, plan_from_expansion,
};
use crate::core::prompts::{expansion_prompt, fast_prompt, sub_search_prompt, synthesis_prompt};
use crate::engines::{Engine, EngineJob};
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
        }
    }
}

pub fn spawn_search(engine: Arc<dyn Engine>, request: SearchRequest) -> SearchHandle {
    let (tx, events) = mpsc::channel(EVENT_BUFFER);
    let task = tokio::spawn(run_search(engine, request, tx));
    SearchHandle::new(events, task)
}

pub async fn run_search(
    engine: Arc<dyn Engine>,
    request: SearchRequest,
    tx: mpsc::Sender<SearchEvent>,
) {
    match run_stages(engine.as_ref(), &request, &tx).await {
        Ok(()) => send(&tx, SearchEvent::Completed).await,
        Err(message) => send(&tx, SearchEvent::Failed(message)).await,
    }
}

async fn send(tx: &mpsc::Sender<SearchEvent>, event: SearchEvent) {
    let _ = tx.send(event).await;
}

async fn run_stages(
    engine: &dyn Engine,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<(), String> {
    if request.fast {
        return run_fast_stages(engine, request, tx).await;
    }
    let plan = expansion_stage(engine, request).await;
    send(tx, SearchEvent::PlanReady(plan.clone())).await;
    let sub_results = fanout_stage(engine, &plan, request, tx).await;
    if sub_results.is_empty() {
        return Err("every sub-query failed; nothing to synthesize".to_string());
    }
    let merged = merge_sub_results(&sub_results);
    if merged.findings.is_empty() {
        return Err("the searches produced no findings with usable sources".to_string());
    }
    send(tx, SearchEvent::SynthesisStarted).await;
    let answer = synthesis_stage(engine, &plan, &merged, request).await?;
    send(tx, SearchEvent::AnswerReady(Box::new(answer.clone()))).await;
    link_validation_stage(&answer, request, tx).await;
    image_fetch_stage(&answer, request, tx).await;
    Ok(())
}

async fn run_fast_stages(
    engine: &dyn Engine,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Result<(), String> {
    let plan = literal_plan(&request.query, request.mode, &request.answer_lang);
    send(tx, SearchEvent::PlanReady(plan)).await;
    send(tx, SearchEvent::SubQueryStarted { idx: 0 }).await;
    let result = fast_answer_stage(engine, request).await;
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

async fn fast_answer_stage(engine: &dyn Engine, request: &SearchRequest) -> Result<Answer, String> {
    let inline_schema = !engine.supports_json_schema();
    let job = EngineJob {
        prompt: fast_prompt(
            &request.query,
            request.mode.spec(),
            &request.answer_lang,
            inline_schema,
        ),
        schema: Some(FAST_ANSWER_SCHEMA),
        timeout: request.fast_timeout,
    };
    let output = engine
        .run(&job)
        .await
        .map_err(|error| format!("fast search failed: {error}"))?;
    let value = extract_json(&output.text)
        .ok_or_else(|| "fast search returned no parsable JSON".to_string())?;
    let answer = parse_answer(value)
        .map_err(|error| format!("fast search JSON did not match the answer schema: {error}"))?;
    let allowed = self_declared_urls(&answer);
    Ok(renumber_sources(strip_image_blocks(answer), &allowed))
}

async fn expansion_stage(engine: &dyn Engine, request: &SearchRequest) -> SearchPlan {
    let prompt = expansion_prompt(
        &request.query,
        request.mode.spec(),
        &request.answer_lang,
        request.breadth,
    );
    let job = EngineJob {
        prompt,
        schema: None,
        timeout: EXPANSION_TIMEOUT,
    };
    let expansion = match engine.run(&job).await {
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

async fn fanout_stage(
    engine: &dyn Engine,
    plan: &SearchPlan,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Vec<SubResult> {
    let mode_spec = plan.mode.spec();
    let sub_query_futures: Vec<_> = plan
        .sub_queries
        .iter()
        .enumerate()
        .map(|(idx, sub)| track_sub_query(engine, idx, sub, mode_spec, request, tx))
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
    sub: &SubQuery,
    mode_spec: &ModeSpec,
    request: &SearchRequest,
    tx: &mpsc::Sender<SearchEvent>,
) -> Option<SubResult> {
    send(tx, SearchEvent::SubQueryStarted { idx }).await;
    let result = run_sub_query(engine, sub, mode_spec, request.engine_timeout).await;
    let ok = result.is_some();
    send(tx, SearchEvent::SubQueryFinished { idx, ok }).await;
    result
}

async fn run_sub_query(
    engine: &dyn Engine,
    sub: &SubQuery,
    mode_spec: &ModeSpec,
    timeout: Duration,
) -> Option<SubResult> {
    let job = EngineJob {
        prompt: sub_search_prompt(sub, mode_spec),
        schema: None,
        timeout,
    };
    let output = engine.run(&job).await.ok()?;
    let value = extract_json(&output.text)?;
    let response = parse_sub_response(&value)?;
    Some(SubResult {
        query: sub.query.clone(),
        lang: sub.lang.clone(),
        response,
    })
}

async fn synthesis_stage(
    engine: &dyn Engine,
    plan: &SearchPlan,
    merged: &MergedFindings,
    request: &SearchRequest,
) -> Result<Answer, String> {
    let inline_schema = !engine.supports_json_schema();
    let job = EngineJob {
        prompt: synthesis_prompt(plan, merged, inline_schema),
        schema: Some(ANSWER_SCHEMA),
        timeout: request.engine_timeout,
    };
    let output = engine
        .run(&job)
        .await
        .map_err(|error| format!("synthesis failed: {error}"))?;
    let value = extract_json(&output.text)
        .ok_or_else(|| "synthesis returned no parsable JSON".to_string())?;
    let answer = parse_answer(value)
        .map_err(|error| format!("synthesis JSON did not match the answer schema: {error}"))?;
    let answer = eject_unknown_images(answer, &allowed_image_urls(merged));
    Ok(renumber_sources(answer, &allowed_urls(merged)))
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
    use crate::engines::{BoxedEngineFuture, EngineError, EngineOutput};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeEngine {
        fail_markers: Vec<&'static str>,
        simple_expansion: bool,
        calls: AtomicUsize,
    }

    impl FakeEngine {
        fn reliable() -> Self {
            Self {
                fail_markers: vec![],
                simple_expansion: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn failing_on(markers: &[&'static str]) -> Self {
            Self {
                fail_markers: markers.to_vec(),
                simple_expansion: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn rating_simple() -> Self {
            Self {
                fail_markers: vec![],
                simple_expansion: true,
                calls: AtomicUsize::new(0),
            }
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
                let marker = marker_of(&job.prompt);
                if self.fail_markers.contains(&marker) {
                    return Err(EngineError::Reported(format!("forced failure: {marker}")));
                }
                Ok(EngineOutput {
                    text: self.canned_response(marker),
                })
            })
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
        }
    }

    fn fast_request() -> SearchRequest {
        SearchRequest {
            fast: true,
            ..request()
        }
    }

    async fn collect_events(engine: FakeEngine) -> Vec<SearchEvent> {
        drain(engine, request()).await.1
    }

    async fn drain(engine: FakeEngine, request: SearchRequest) -> (usize, Vec<SearchEvent>) {
        let engine = Arc::new(engine);
        let mut handle = spawn_search(engine.clone(), request);
        let mut events = Vec::new();
        while let Some(event) = handle.events.recv().await {
            events.push(event);
        }
        (engine.calls.load(Ordering::SeqCst), events)
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
