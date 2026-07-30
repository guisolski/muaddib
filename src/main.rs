use clap::Parser;
use faro::config_store;
use faro::core::config::Config;
use faro::core::mode::Mode;
use faro::engines::cli::CliEngine;
use faro::engines::{EngineStatus, choose_engine, detect_engines};
use faro::pipeline::search::{SearchRequest, spawn_search};
use faro::pipeline::{LinkStatus, SearchEvent};
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(
    name = "faro",
    version,
    about = "AI-powered meta-search for your terminal"
)]
struct Cli {
    query: Option<String>,

    #[arg(long, help = "Search mode: general, scientific, news, or deep")]
    mode: Option<Mode>,

    #[arg(long, help = "Engine CLI to use for this run")]
    engine: Option<String>,

    #[arg(long, help = "Answer language for this run, as a BCP-47 tag")]
    lang: Option<String>,

    #[arg(long, help = "Run headless and print the answer JSON to stdout")]
    print: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let (config, config_notice) = config_store::load_or_default();
    let config = apply_cli_overrides(config, &cli);
    if let Some(notice) = config_notice {
        eprintln!("faro: {notice}");
    }
    let statuses = detect_engines(&config);
    if cli.print {
        run_headless(&cli, &config, &statuses).await
    } else {
        eprintln!("faro: the TUI is not wired yet; run with --print QUERY");
        ExitCode::from(2)
    }
}

fn apply_cli_overrides(mut config: Config, cli: &Cli) -> Config {
    if let Some(engine) = &cli.engine {
        config.engine.clone_from(engine);
    }
    if let Some(lang) = &cli.lang {
        config.language.clone_from(lang);
    }
    config
}

async fn run_headless(cli: &Cli, config: &Config, statuses: &[EngineStatus]) -> ExitCode {
    let Some(query) = cli.query.clone() else {
        eprintln!("faro: --print requires a query argument");
        return ExitCode::from(2);
    };
    let Ok((status, engine_notice)) = choose_engine(statuses, &config.engine) else {
        report_missing_engines(statuses);
        return ExitCode::FAILURE;
    };
    if let Some(notice) = engine_notice {
        eprintln!("faro: {notice}");
    }
    let engine = CliEngine::from_status(status).expect("an available engine has a resolved path");
    let mode = cli.mode.unwrap_or(Mode::General);
    let request = SearchRequest::from_config(query, mode, config);
    stream_search_to_stdio(Arc::new(engine), request).await
}

fn report_missing_engines(statuses: &[EngineStatus]) {
    eprintln!("faro: no supported engine CLI is installed. Install one of:");
    for status in statuses {
        eprintln!("  {:<14} {}", status.spec.name, status.spec.install_hint);
    }
}

async fn stream_search_to_stdio(engine: Arc<CliEngine>, request: SearchRequest) -> ExitCode {
    let mut handle = spawn_search(engine, request);
    let mut answer = None;
    let mut failure = None;
    while let Some(event) = handle.events.recv().await {
        match event {
            SearchEvent::AnswerReady(ready) => answer = Some(ready),
            SearchEvent::Failed(message) => failure = Some(message),
            other => report_progress(&other),
        }
    }
    if let Some(answer) = answer {
        let json =
            serde_json::to_string_pretty(answer.as_ref()).expect("an answer always serializes");
        println!("{json}");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "faro: search failed: {}",
            failure.unwrap_or_else(|| "no answer produced".to_string())
        );
        ExitCode::FAILURE
    }
}

fn report_progress(event: &SearchEvent) {
    match event {
        SearchEvent::PlanReady(plan) => {
            eprintln!("faro: searching {} sub-queries:", plan.sub_queries.len());
            for sub in &plan.sub_queries {
                eprintln!("  [{}] {}", sub.lang, sub.query);
            }
        }
        SearchEvent::SubQueryFinished { idx, ok } => {
            let outcome = if *ok { "done" } else { "failed" };
            eprintln!("faro: sub-query {} {outcome}", idx + 1);
        }
        SearchEvent::SynthesisStarted => eprintln!("faro: synthesizing answer..."),
        SearchEvent::LinkChecked { source_id, status } => {
            eprintln!("faro: link [{source_id}] {}", link_status_label(*status));
        }
        SearchEvent::Completed
        | SearchEvent::SubQueryStarted { .. }
        | SearchEvent::AnswerReady(_)
        | SearchEvent::Failed(_) => {}
    }
}

fn link_status_label(status: LinkStatus) -> String {
    match status {
        LinkStatus::Valid => "ok".to_string(),
        LinkStatus::Invalid(code) => format!("broken (HTTP {code})"),
        LinkStatus::Unreachable => "unreachable".to_string(),
    }
}
