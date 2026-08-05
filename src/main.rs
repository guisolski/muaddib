use clap::Parser;
use muaddib::config_store;
use muaddib::core::config::Config;
use muaddib::core::cost::{EngineUsage, usage_label};
use muaddib::core::mode::Mode;
use muaddib::engines::cli::CliEngine;
use muaddib::engines::{EngineSpec, EngineStatus, choose_engine, detect_engines};
use muaddib::history_store;
use muaddib::pipeline::search::{SearchRequest, spawn_search};
use muaddib::pipeline::{LinkStatus, SearchEvent};
use muaddib::tree_store;
use muaddib::tui::SessionStart;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Debug, Parser)]
#[command(
    name = "muaddib",
    version,
    about = "AI-powered meta-search for your terminal"
)]
struct Cli {
    query: Option<String>,

    #[arg(
        long,
        help = "Search mode: general, scientific, news, code, forums, deep, or exhaustive"
    )]
    mode: Option<Mode>,

    #[arg(long, help = "Engine CLI to use for this run")]
    engine: Option<String>,

    #[arg(long, help = "Model passed to the engine CLI for this run")]
    model: Option<String>,

    #[arg(long, help = "Answer language for this run, as a BCP-47 tag")]
    lang: Option<String>,

    #[arg(
        long,
        help = "Answer from a single engine call, trading depth for speed"
    )]
    fast: bool,

    #[arg(long, help = "Disable the built-in web-search grounding for this run")]
    no_websearch: bool,

    #[arg(long, help = "Run headless and print the answer JSON to stdout")]
    print: bool,

    #[arg(long, help = "Reopen a saved research session file in the TUI")]
    session: Option<PathBuf>,

    #[arg(long, help = "Erase the saved search history and exit")]
    clear_history: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.clear_history {
        return clear_history();
    }
    let (config, config_notice) = config_store::load_or_default();
    let config = apply_cli_overrides(config, &cli);
    if let Some(notice) = config_notice {
        eprintln!("muaddib: {notice}");
    }
    let statuses = detect_engines(&config);
    if cli.print {
        run_headless(&cli, &config, &statuses).await
    } else {
        let session = load_session(cli.session.as_deref());
        match muaddib::tui::run(
            config,
            statuses,
            cli.query.clone(),
            cli.mode,
            cli.fast,
            session,
        )
        .await
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("muaddib: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

fn load_session(path: Option<&Path>) -> Option<SessionStart> {
    let path = path?;
    match tree_store::load(path) {
        Ok(session) => Some(SessionStart {
            tree: session.tree,
            path: path.to_path_buf(),
        }),
        Err(error) => {
            eprintln!("muaddib: {error}");
            None
        }
    }
}

fn clear_history() -> ExitCode {
    let count = history_store::load_recall().len();
    match history_store::clear() {
        Ok(()) => {
            eprintln!("muaddib: search history cleared ({count} entries)");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("muaddib: failed to clear history: {error}");
            ExitCode::FAILURE
        }
    }
}

fn apply_cli_overrides(mut config: Config, cli: &Cli) -> Config {
    if let Some(engine) = &cli.engine {
        config.engine.clone_from(engine);
    }
    if let Some(model) = &cli.model {
        let engine = config.engine.clone();
        config.set_model_override(&engine, Some(model.clone()));
    }
    if let Some(lang) = &cli.lang {
        config.language.clone_from(lang);
    }
    if cli.no_websearch {
        config.websearch.enabled = false;
    }
    config
}

async fn run_headless(cli: &Cli, config: &Config, statuses: &[EngineStatus]) -> ExitCode {
    let Some(query) = cli.query.clone() else {
        eprintln!("muaddib: --print requires a query argument");
        return ExitCode::from(2);
    };
    let Ok((status, engine_notice)) = choose_engine(statuses, &config.engine) else {
        report_missing_engines(statuses);
        return ExitCode::FAILURE;
    };
    if let Some(notice) = engine_notice {
        eprintln!("muaddib: {notice}");
    }
    let engine = CliEngine::from_status(status)
        .expect("an available engine has a resolved path")
        .with_model(headless_model(config, status.spec, cli.fast));
    let mode = cli.mode.unwrap_or(Mode::General);
    let request = SearchRequest {
        fetch_images: false,
        ..SearchRequest::from_config(query.clone(), mode, cli.fast, config)
    };
    record_history(&query, mode, cli.fast);
    stream_search_to_stdio(Arc::new(engine), request).await
}

fn headless_model(config: &Config, spec: &EngineSpec, fast: bool) -> Option<String> {
    if fast && let Some(model) = config.fast_model_override(spec.name).or(spec.fast_model) {
        return Some(model.to_string());
    }
    config.model_override(spec.name).map(str::to_string)
}

fn record_history(query: &str, mode: Mode, fast: bool) {
    let entry = history_store::stamped_entry(query, mode, fast);
    if let Err(error) = history_store::append(&entry) {
        eprintln!("muaddib: failed to save history: {error}");
    }
}

fn report_missing_engines(statuses: &[EngineStatus]) {
    eprintln!("muaddib: no supported engine CLI is installed. Install one of:");
    for status in statuses {
        eprintln!("  {:<14} {}", status.spec.name, status.spec.install_hint);
    }
}

async fn stream_search_to_stdio(engine: Arc<CliEngine>, request: SearchRequest) -> ExitCode {
    let mut handle = spawn_search(engine, request);
    let mut answer: Option<Box<muaddib::core::answer::Answer>> = None;
    let mut failure = None;
    let mut usage = EngineUsage::default();
    while let Some(event) = handle.events.recv().await {
        match event {
            SearchEvent::AnswerReady(ready) => answer = Some(ready),
            SearchEvent::Failed(message) => failure = Some(message),
            SearchEvent::CallCosted { usage: call } => usage = usage.plus(call),
            other => {
                report_progress(&other);
                if let (SearchEvent::LinkChecked { source_id, status }, Some(answer)) =
                    (&other, answer.as_mut())
                {
                    record_link_status(answer, *source_id, *status);
                }
            }
        }
    }
    if let Some(label) = usage_label(usage) {
        eprintln!("muaddib: {label}");
    }
    if let Some(answer) = answer {
        let json =
            serde_json::to_string_pretty(answer.as_ref()).expect("an answer always serializes");
        println!("{json}");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "muaddib: search failed: {}",
            failure.unwrap_or_else(|| "no answer produced".to_string())
        );
        ExitCode::FAILURE
    }
}

fn record_link_status(
    answer: &mut muaddib::core::answer::Answer,
    source_id: u32,
    status: LinkStatus,
) {
    if let Some(source) = answer
        .sources
        .iter_mut()
        .find(|source| source.id == source_id)
    {
        source.status = Some(status);
    }
}

fn report_progress(event: &SearchEvent) {
    match event {
        SearchEvent::PlanReady(plan) => {
            eprintln!("muaddib: searching {} sub-queries:", plan.sub_queries.len());
            for sub in &plan.sub_queries {
                eprintln!("  [{}] {}", sub.lang, sub.query);
            }
        }
        SearchEvent::WebHits { count, .. } => eprintln!("muaddib: web hits: {count}"),
        SearchEvent::PageFetched { url, ok } => {
            let outcome = if *ok { "fetched" } else { "skipped" };
            eprintln!("muaddib: page {url} {outcome}");
        }
        SearchEvent::SubQueryFinished { idx, ok } => {
            let outcome = if *ok { "done" } else { "failed" };
            eprintln!("muaddib: sub-query {} {outcome}", idx + 1);
        }
        SearchEvent::EngineActivity { label, target } => {
            eprintln!("muaddib: {label} {target}");
        }
        SearchEvent::FastFellBack { reason } => {
            eprintln!("muaddib: {reason}; running the full search instead");
        }
        SearchEvent::SynthesisStarted => eprintln!("muaddib: synthesizing answer..."),
        SearchEvent::ReflectionStarted => eprintln!("muaddib: reviewing the draft..."),
        SearchEvent::ReflectionGaps { gaps } => {
            eprintln!("muaddib: {} gaps found", gaps.len());
            for gap in gaps {
                eprintln!("  [{}] {}", gap.lang, gap.query);
            }
        }
        SearchEvent::LinkChecked { source_id, status } => {
            eprintln!("muaddib: link [{source_id}] {}", link_status_label(*status));
        }
        SearchEvent::Completed
        | SearchEvent::SubQueryStarted { .. }
        | SearchEvent::CallCosted { .. }
        | SearchEvent::AnswerReady(_)
        | SearchEvent::ImageFetched { .. }
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
