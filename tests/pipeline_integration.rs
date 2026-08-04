use muaddib::core::config::WebSearchConfig;
use muaddib::core::context::ResearchContext;
use muaddib::core::mode::Mode;
use muaddib::engines::cli::CliEngine;
use muaddib::engines::engine_by_name;
use muaddib::pipeline::SearchEvent;
use muaddib::pipeline::search::{SearchRequest, spawn_search};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fake_engine(script: &str) -> Arc<CliEngine> {
    let spec = engine_by_name("claude").expect("claude spec exists");
    Arc::new(CliEngine::new(spec, fixture_path(script)))
}

fn request() -> SearchRequest {
    SearchRequest {
        query: "rust async runtimes".to_string(),
        mode: Mode::General,
        answer_lang: "en".to_string(),
        breadth: 4,
        max_parallel: 4,
        engine_timeout: Duration::from_secs(20),
        fast: false,
        fast_timeout: Duration::from_secs(20),
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

async fn collect_events(script: &str) -> Vec<SearchEvent> {
    drain(script, request()).await
}

async fn drain(script: &str, request: SearchRequest) -> Vec<SearchEvent> {
    let mut handle = spawn_search(fake_engine(script), request);
    let mut events = Vec::new();
    while let Some(event) = handle.events.recv().await {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn subprocess_pipeline_completes_with_a_cited_answer() {
    let events = collect_events("fake-engine.sh").await;
    assert!(matches!(events.first(), Some(SearchEvent::PlanReady(_))));
    assert!(matches!(events.last(), Some(SearchEvent::Completed)));
    let answer = events
        .iter()
        .find_map(|event| match event {
            SearchEvent::AnswerReady(answer) => Some(answer),
            _ => None,
        })
        .expect("pipeline produced an answer");
    assert_eq!(answer.title, "Rust async runtimes");
    let urls: Vec<&str> = answer
        .sources
        .iter()
        .map(|source| source.url.as_str())
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://tokio.rs/tokio/tutorial",
            "https://github.com/smol-rs/smol",
        ]
    );
}

#[tokio::test]
async fn subprocess_pipeline_runs_every_planned_sub_query() {
    let events = collect_events("fake-engine.sh").await;
    let Some(SearchEvent::PlanReady(plan)) = events.first() else {
        panic!("expected PlanReady first");
    };
    assert_eq!(plan.sub_queries.len(), 4);
    let finished_ok = events
        .iter()
        .filter(|event| matches!(event, SearchEvent::SubQueryFinished { ok: true, .. }))
        .count();
    assert_eq!(finished_ok, 4);
}

#[tokio::test]
async fn subprocess_pipeline_survives_a_failing_expansion_call() {
    let events = collect_events("fake-engine-fail-expand.sh").await;
    let Some(SearchEvent::PlanReady(plan)) = events.first() else {
        panic!("expected PlanReady first");
    };
    assert_eq!(plan.sub_queries[0].query, "rust async runtimes");
    assert_eq!(plan.sub_queries[0].rationale, "literal query");
    assert!(matches!(events.last(), Some(SearchEvent::Completed)));
}

#[tokio::test]
async fn subprocess_pipeline_ejects_sources_absent_from_findings() {
    let events = collect_events("fake-engine.sh").await;
    let answer = events
        .iter()
        .find_map(|event| match event {
            SearchEvent::AnswerReady(answer) => Some(answer),
            _ => None,
        })
        .expect("pipeline produced an answer");
    assert!(
        answer
            .sources
            .iter()
            .all(|source| !source.url.contains("invented.example"))
    );
    let cited_paragraphs = answer
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                block,
                muaddib::core::answer::Block::Paragraph { source_ids, .. } if !source_ids.is_empty()
            )
        })
        .count();
    assert_eq!(cited_paragraphs, 2);
}

#[tokio::test]
async fn fast_mode_answers_from_one_subprocess_call() {
    let events = drain("fake-engine.sh", fast_request()).await;
    assert!(matches!(events.first(), Some(SearchEvent::PlanReady(_))));
    assert!(matches!(events.last(), Some(SearchEvent::Completed)));
    let Some(SearchEvent::PlanReady(plan)) = events.first() else {
        panic!("expected PlanReady first");
    };
    assert_eq!(plan.sub_queries.len(), 1);
    assert_eq!(plan.sub_queries[0].query, "rust async runtimes");
    let started = events
        .iter()
        .filter(|event| matches!(event, SearchEvent::SubQueryStarted { .. }))
        .count();
    assert_eq!(started, 1);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SearchEvent::SynthesisStarted))
    );
}

#[tokio::test]
async fn fast_mode_strips_images_and_undeclared_sources() {
    let events = drain("fake-engine.sh", fast_request()).await;
    let answer = events
        .iter()
        .find_map(|event| match event {
            SearchEvent::AnswerReady(answer) => Some(answer),
            _ => None,
        })
        .expect("pipeline produced an answer");
    assert_eq!(answer.title, "Rust async runtimes");
    let urls: Vec<&str> = answer
        .sources
        .iter()
        .map(|source| source.url.as_str())
        .collect();
    assert_eq!(urls, vec!["https://tokio.rs/tokio/tutorial"]);
    assert!(
        !answer
            .blocks
            .iter()
            .any(|block| matches!(block, muaddib::core::answer::Block::Image { .. }))
    );
}
