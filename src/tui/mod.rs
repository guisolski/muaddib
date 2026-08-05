pub mod anim;
pub mod app;
pub mod event;
pub mod images;
pub mod keymap;
pub mod search_state;
pub mod theme;
pub mod update;
pub mod view;
pub mod widgets;

use crate::config_store;
use crate::core::config::Config;
use crate::core::context::context_for;
use crate::core::export::osc52_payload;
use crate::core::history::{push_recall, repeats_latest};
use crate::core::mode::Mode;
use crate::core::tree::ResearchTree;
use crate::engines::cli::CliEngine;
use crate::engines::{EngineSpec, EngineStatus, choose_engine};
use crate::history_store;
use crate::pipeline::SearchEvent;
use crate::pipeline::search::{SearchRequest, spawn_search};
use crate::tree_store;
use crate::tui::app::{App, Screen};
use crate::tui::event::{AppEvent, Command};
use crate::tui::update::update;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::Receiver;

const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub struct SessionStart {
    pub tree: ResearchTree,
    pub path: std::path::PathBuf,
}

pub async fn run(
    config: Config,
    statuses: Vec<EngineStatus>,
    initial_query: Option<String>,
    initial_mode: Option<Mode>,
    fast: bool,
    session: Option<SessionStart>,
) -> anyhow::Result<()> {
    let mut app = App::new(config, statuses, initial_mode, fast);
    app.history = history_store::load_recall();
    app.clock_unix = unix_seconds();
    if let Some(session) = session {
        app.tree = session.tree;
        app.session_path = Some(session.path);
        if app.tree.current.is_none() {
            app.tree.current = app.tree.nodes.last().map(|node| node.id);
        }
        if let Some(node) = app.tree.current_node() {
            app.search.answer = Some(node.answer.clone());
            app.screen = Screen::Results;
        }
    }
    if let Some(query) = initial_query {
        app.input = tui_input::Input::new(query.clone());
        start_search(&mut app, &query);
    }
    let mut terminal = ratatui::init();
    if let Ok(size) = terminal.size() {
        app.viewport = app::Viewport {
            width: size.width,
            height: size.height,
        };
    }
    if app.config.images {
        app.image_runtime.picker = Some(
            ratatui_image::picker::Picker::from_query_stdio()
                .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks()),
        );
    }
    let result = event_loop(&mut terminal, &mut app).await;
    ratatui::restore();
    result
}

async fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    let mut terminal_events = EventStream::new();
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    loop {
        app.clock_unix = unix_seconds();
        terminal.draw(|frame| view::draw(frame, app))?;
        let app_event = tokio::select! {
            maybe_event = terminal_events.next() => {
                match translate_terminal_event(maybe_event) {
                    Some(event) => event,
                    None => continue,
                }
            }
            _ = ticker.tick() => AppEvent::Tick,
            search_event = next_search_event(app.search.events_mut()) => {
                if let Some(event) = search_event {
                    AppEvent::Search(event)
                } else {
                    app.end_search();
                    continue;
                }
            }
        };
        if let Some(command) = update(app, app_event)
            && dispatch_command(app, command)
        {
            return Ok(());
        }
    }
}

fn translate_terminal_event(
    maybe_event: Option<Result<Event, std::io::Error>>,
) -> Option<AppEvent> {
    match maybe_event? {
        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Some(AppEvent::Key(key)),
        Ok(Event::Resize(width, height)) => Some(AppEvent::Resize { width, height }),
        _ => None,
    }
}

async fn next_search_event(events: Option<&mut Receiver<SearchEvent>>) -> Option<SearchEvent> {
    match events {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

fn dispatch_command(app: &mut App, command: Command) -> bool {
    match command {
        Command::Quit => return true,
        Command::StartSearch { query } => start_search(app, &query),
        Command::CancelSearch => app.end_search(),
        Command::OpenUrl(url) => open_url(&url),
        Command::SaveConfig => save_config(app),
        Command::SaveSession => save_session(app),
        Command::CopyAnswer(markdown) => copy_answer(app, &markdown),
        Command::ExportAnswer { filename, contents } => export_answer(app, &filename, &contents),
        Command::ClearHistory => clear_history(app),
    }
    false
}

const CLIPBOARD_COMMANDS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

fn copy_answer(app: &mut App, markdown: &str) {
    if copy_with_command(markdown) || copy_with_osc52(markdown) {
        app.notice = Some("answer copied as markdown".to_string());
    } else {
        app.notice = Some("could not reach a clipboard".to_string());
    }
}

fn copy_with_command(markdown: &str) -> bool {
    CLIPBOARD_COMMANDS
        .iter()
        .any(|(bin, args)| pipe_to_command(bin, args, markdown))
}

fn pipe_to_command(bin: &str, args: &[&str], markdown: &str) -> bool {
    let Ok(mut child) = std::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(stdin) = child.stdin.as_mut()
        && std::io::Write::write_all(stdin, markdown.as_bytes()).is_err()
    {
        return false;
    }
    drop(child.stdin.take());
    child.wait().is_ok_and(|status| status.success())
}

fn copy_with_osc52(markdown: &str) -> bool {
    let Some(payload) = osc52_payload(markdown) else {
        return false;
    };
    let mut stdout = std::io::stdout();
    std::io::Write::write_all(&mut stdout, payload.as_bytes()).is_ok()
        && std::io::Write::flush(&mut stdout).is_ok()
}

fn export_answer(app: &mut App, filename: &str, contents: &str) {
    match std::fs::write(filename, contents) {
        Ok(()) => app.notice = Some(format!("answer exported to {filename}")),
        Err(error) => app.notice = Some(format!("failed to export: {error}")),
    }
}

fn save_session(app: &mut App) {
    match tree_store::save(&app.tree, app.session_path.as_deref()) {
        Ok(path) => {
            app.notice = Some(format!("session saved to {}", path.display()));
            app.session_path = Some(path);
        }
        Err(error) => app.notice = Some(format!("failed to save session: {error}")),
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn start_search(app: &mut App, query: &str) {
    match choose_engine(&app.statuses, &app.config.engine) {
        Err(error) => app.notice = Some(error.to_string()),
        Ok((status, notice)) => {
            let Some(engine) = CliEngine::from_status(status) else {
                return;
            };
            let engine = engine.with_model(search_model(&app.config, status.spec, app.fast));
            let context = app
                .pending_parent
                .map(|parent| context_for(&app.tree, parent))
                .unwrap_or_default();
            let request = SearchRequest {
                context,
                ..SearchRequest::from_config(
                    query.to_string(),
                    app.current_mode(),
                    app.fast,
                    &app.config,
                )
            };
            app.begin_search();
            app.notice = notice;
            record_history(app, query);
            app.search.handle = Some(spawn_search(Arc::new(engine), request));
        }
    }
}

fn search_model(config: &Config, spec: &EngineSpec, fast: bool) -> Option<String> {
    if fast && let Some(model) = config.fast_model_override(spec.name).or(spec.fast_model) {
        return Some(model.to_string());
    }
    config.model_override(spec.name).map(str::to_string)
}

fn record_history(app: &mut App, query: &str) {
    app.history_idx = None;
    let repeat = repeats_latest(&app.history, query);
    push_recall(&mut app.history, query);
    if repeat {
        return;
    }
    let entry = history_store::stamped_entry(query, app.current_mode(), app.fast);
    if let Err(error) = history_store::append(&entry) {
        app.notice = Some(format!("failed to save history: {error}"));
    }
}

fn clear_history(app: &mut App) {
    let count = app.history.len();
    match history_store::clear() {
        Ok(()) => {
            app.history.clear();
            app.history_draft.clear();
            app.history_idx = None;
            app.notice = Some(format!("search history cleared ({count} entries)"));
        }
        Err(error) => app.notice = Some(format!("failed to clear history: {error}")),
    }
}

fn save_config(app: &mut App) {
    match config_store::save(&app.config) {
        Ok(()) => app.notice = Some("config saved".to_string()),
        Err(error) => app.notice = Some(format!("failed to save config: {error}")),
    }
}

fn open_url(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
