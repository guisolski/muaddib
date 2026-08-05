use muaddib::config_store;
use muaddib::core::answer::Answer;
use muaddib::core::cost::EngineUsage;
use muaddib::core::eval::{
    CaseReport, EvalCase, parse_suite, report_markdown, score_answer, summarize,
};
use muaddib::engines::cli::CliEngine;
use muaddib::engines::{choose_engine, detect_engines};
use muaddib::pipeline::SearchEvent;
use muaddib::pipeline::search::{SearchRequest, spawn_search};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn isolate_state() {
    let scratch = std::env::temp_dir().join("muaddib-eval");
    std::fs::create_dir_all(&scratch).expect("scratch dir is writable");
    unsafe {
        std::env::set_var("MUADDIB_HISTORY", scratch.join("history.jsonl"));
        std::env::set_var("MUADDIB_SESSIONS", scratch.join("sessions"));
    }
}

struct Outcome {
    answer: Option<Answer>,
    usage: EngineUsage,
    elapsed_ms: u64,
}

async fn run_case(case: &EvalCase) -> Outcome {
    let (config, _) = config_store::load_or_default();
    let statuses = detect_engines(&config);
    let (status, _) = choose_engine(&statuses, &config.engine).expect("an engine CLI is installed");
    let engine = CliEngine::from_status(status)
        .expect("an available engine has a resolved path")
        .with_model(config.model_override(status.spec.name).map(str::to_string));
    let request = SearchRequest {
        fetch_images: false,
        ..SearchRequest::from_config(case.query.clone(), case.mode, false, &config)
    };

    let started = Instant::now();
    let mut handle = spawn_search(Arc::new(engine), request);
    let mut answer: Option<Answer> = None;
    let mut usage = EngineUsage::default();
    while let Some(event) = handle.events.recv().await {
        match event {
            SearchEvent::AnswerReady(ready) => answer = Some(*ready),
            SearchEvent::CallCosted { usage: call } => usage = usage.plus(call),
            SearchEvent::LinkChecked { source_id, status } => {
                if let Some(answer) = answer.as_mut()
                    && let Some(source) = answer
                        .sources
                        .iter_mut()
                        .find(|source| source.id == source_id)
                {
                    source.status = Some(status);
                }
            }
            SearchEvent::Failed(message) => eprintln!("  failed: {message}"),
            _ => {}
        }
    }
    Outcome {
        answer,
        usage,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls a real AI CLI: run with `make eval`"]
async fn evaluate_the_golden_queries_against_a_real_engine() {
    isolate_state();
    let suite = parse_suite(
        &std::fs::read_to_string(repo_path("tests/fixtures/eval/cases.toml"))
            .expect("the golden suite is readable"),
    )
    .expect("the golden suite parses");

    let mut reports: Vec<CaseReport> = Vec::new();
    let mut failures = 0;
    for case in &suite.cases {
        eprintln!("eval: {} [{}]", case.query, case.mode);
        let outcome = run_case(case).await;
        match outcome.answer {
            Some(answer) => reports.push(score_answer(
                &answer,
                case,
                outcome.elapsed_ms,
                Some(&outcome.usage),
            )),
            None => failures += 1,
        }
    }

    let summary = summarize(&reports, failures);
    let markdown = report_markdown(&summary, &reports);
    let out = repo_path("docs/eval-baseline.md");
    std::fs::write(&out, &markdown).expect("the baseline is writable");
    eprintln!("\n{markdown}");
    eprintln!("eval: wrote {}", out.display());

    assert!(!reports.is_empty(), "every golden query failed to answer");
}
