pub mod app;
pub mod event;
pub mod keymap;
pub mod theme;
pub mod update;
pub mod view;
pub mod widgets;

use crate::config_store;
use crate::core::config::Config;
use crate::core::mode::Mode;
use crate::engines::cli::CliEngine;
use crate::engines::{EngineStatus, choose_engine};
use crate::pipeline::SearchEvent;
use crate::pipeline::search::{SearchRequest, spawn_search};
use crate::tui::app::App;
use crate::tui::event::{AppEvent, Command};
use crate::tui::update::update;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

const TICK_INTERVAL: Duration = Duration::from_millis(100);

pub async fn run(
    config: Config,
    statuses: Vec<EngineStatus>,
    initial_query: Option<String>,
    initial_mode: Option<Mode>,
) -> anyhow::Result<()> {
    let mut app = App::new(config, statuses, initial_mode);
    if let Some(query) = initial_query {
        app.input = tui_input::Input::new(query.clone());
        start_search(&mut app, query);
    }
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app).await;
    ratatui::restore();
    result
}

async fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    let mut terminal_events = EventStream::new();
    let mut ticker = tokio::time::interval(TICK_INTERVAL);
    loop {
        terminal.draw(|frame| view::draw(frame, app))?;
        let app_event = tokio::select! {
            maybe_event = terminal_events.next() => {
                match translate_terminal_event(maybe_event) {
                    Some(event) => event,
                    None => continue,
                }
            }
            _ = ticker.tick() => AppEvent::Tick,
            search_event = next_search_event(app.search.as_mut().map(|handle| &mut handle.events)) => {
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
        Ok(Event::Resize(..)) => Some(AppEvent::Resize),
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
        Command::StartSearch { query } => start_search(app, query),
        Command::CancelSearch => app.end_search(),
        Command::OpenUrl(url) => open_url(&url),
        Command::SaveConfig => save_config(app),
    }
    false
}

fn start_search(app: &mut App, query: String) {
    match choose_engine(&app.statuses, &app.config.engine) {
        Err(error) => app.notice = Some(error.to_string()),
        Ok((status, notice)) => {
            let Some(engine) = CliEngine::from_status(status) else {
                return;
            };
            let request = SearchRequest::from_config(query, app.current_mode(), &app.config);
            app.begin_search();
            app.notice = notice;
            app.search = Some(spawn_search(Arc::new(engine), request));
        }
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
