use crate::core::mode::MODES;
use crate::pipeline::SearchEvent;
use crate::tui::app::{App, ConfigField, ConfigForm, LANGUAGES, Overlay, Screen, SubQueryState};
use crate::tui::event::{AppEvent, Command};
use crate::tui::keymap::{Action, Scope, resolve};
use crossterm::event::KeyEvent;

const PAGE_JUMP: u16 = 10;

pub fn update(app: &mut App, event: AppEvent) -> Option<Command> {
    match event {
        AppEvent::Tick => {
            app.tick = app.tick.wrapping_add(1);
            None
        }
        AppEvent::Resize => None,
        AppEvent::Search(search_event) => apply_search_event(app, search_event),
        AppEvent::Key(key) => handle_key(app, &key),
    }
}

fn scope_of(app: &App) -> Scope {
    if app.overlay.is_some() {
        return Scope::Modal;
    }
    match app.screen {
        Screen::Home => Scope::Home,
        Screen::Searching => Scope::Searching,
        Screen::Results => Scope::Results,
    }
}

fn handle_key(app: &mut App, key: &KeyEvent) -> Option<Command> {
    let scope = scope_of(app);
    let Some(action) = resolve(scope, key) else {
        forward_key_to_input(app, scope, key);
        return None;
    };
    perform(app, action)
}

fn forward_key_to_input(app: &mut App, scope: Scope, key: &KeyEvent) {
    if scope == Scope::Home {
        use tui_input::backend::crossterm::EventHandler;
        app.input.handle_event(&crossterm::event::Event::Key(*key));
    }
}

fn perform(app: &mut App, action: Action) -> Option<Command> {
    match action {
        Action::Quit => Some(Command::Quit),
        Action::ToggleHelp => {
            toggle_help(app);
            None
        }
        Action::OpenConfig => {
            app.overlay = Some(Overlay::Config(ConfigForm::from_state(
                &app.config,
                &app.statuses,
            )));
            None
        }
        Action::Back => go_back(app),
        Action::Submit => submit_query(app),
        Action::NextMode => {
            app.mode_idx = (app.mode_idx + 1) % MODES.len();
            None
        }
        Action::PrevMode => {
            app.mode_idx = (app.mode_idx + MODES.len() - 1) % MODES.len();
            None
        }
        Action::ScrollDown => {
            move_down(app);
            None
        }
        Action::ScrollUp => {
            move_up(app);
            None
        }
        Action::PageDown => {
            app.scroll = app.scroll.saturating_add(PAGE_JUMP);
            None
        }
        Action::PageUp => {
            app.scroll = app.scroll.saturating_sub(PAGE_JUMP);
            None
        }
        Action::ScrollTop => {
            app.scroll = 0;
            None
        }
        Action::ScrollBottom => {
            app.scroll = u16::MAX;
            None
        }
        Action::FocusSources => {
            app.sources_focused = !app.sources_focused;
            app.selected_source = 0;
            None
        }
        Action::OpenSelected => open_selected_source(app),
        Action::NewSearch => {
            app.screen = Screen::Home;
            app.input.reset();
            app.answer = None;
            None
        }
        Action::RefineSearch => {
            app.screen = Screen::Home;
            None
        }
        Action::FieldNext | Action::FieldPrev | Action::ValueNext | Action::ValuePrev => {
            edit_config_form(app, action);
            None
        }
        Action::Confirm => confirm_config(app),
    }
}

fn toggle_help(app: &mut App) {
    app.overlay = match app.overlay {
        Some(Overlay::Help) => None,
        _ => Some(Overlay::Help),
    };
}

fn go_back(app: &mut App) -> Option<Command> {
    if app.overlay.is_some() {
        app.overlay = None;
        return None;
    }
    match app.screen {
        Screen::Home => {
            app.input.reset();
            None
        }
        Screen::Searching => {
            app.screen = Screen::Home;
            app.synthesizing = false;
            Some(Command::CancelSearch)
        }
        Screen::Results => {
            app.screen = Screen::Home;
            None
        }
    }
}

fn submit_query(app: &mut App) -> Option<Command> {
    let query = app.input.value().trim().to_string();
    if query.is_empty() {
        return None;
    }
    Some(Command::StartSearch { query })
}

fn move_down(app: &mut App) {
    if app.sources_focused {
        let last = app.source_count().saturating_sub(1);
        app.selected_source = (app.selected_source + 1).min(last);
    } else {
        app.scroll = app.scroll.saturating_add(1);
    }
}

fn move_up(app: &mut App) {
    if app.sources_focused {
        app.selected_source = app.selected_source.saturating_sub(1);
    } else {
        app.scroll = app.scroll.saturating_sub(1);
    }
}

fn open_selected_source(app: &App) -> Option<Command> {
    if !app.sources_focused {
        return None;
    }
    let answer = app.answer.as_ref()?;
    let source = answer.sources.get(app.selected_source)?;
    Some(Command::OpenUrl(source.url.clone()))
}

fn edit_config_form(app: &mut App, action: Action) {
    let statuses_len = app.statuses.len();
    let available: Vec<usize> = app
        .statuses
        .iter()
        .enumerate()
        .filter(|(_, status)| status.available)
        .map(|(index, _)| index)
        .collect();
    let Some(Overlay::Config(form)) = app.overlay.as_mut() else {
        return;
    };
    match action {
        Action::FieldNext => {
            form.field_idx = (form.field_idx + 1) % super::app::CONFIG_FIELDS.len();
        }
        Action::FieldPrev => {
            let len = super::app::CONFIG_FIELDS.len();
            form.field_idx = (form.field_idx + len - 1) % len;
        }
        Action::ValueNext => step_config_value(form, 1, statuses_len, &available),
        Action::ValuePrev => step_config_value(form, -1, statuses_len, &available),
        _ => {}
    }
}

fn step_config_value(form: &mut ConfigForm, step: i8, statuses_len: usize, available: &[usize]) {
    match form.field() {
        ConfigField::Language => {
            form.language_idx = cycle(form.language_idx, LANGUAGES.len(), step);
        }
        ConfigField::Engine => {
            form.engine_idx = next_available_engine(form.engine_idx, statuses_len, available, step);
        }
        ConfigField::ValidateLinks => form.validate_links = !form.validate_links,
        ConfigField::MaxParallel => {
            let raw = i16::from(form.max_parallel) + i16::from(step);
            let clamped = raw.clamp(
                i16::from(crate::core::config::MIN_PARALLEL),
                i16::from(crate::core::config::MAX_PARALLEL),
            );
            form.max_parallel = u8::try_from(clamped).unwrap_or(crate::core::config::MIN_PARALLEL);
        }
    }
}

fn cycle(index: usize, len: usize, step: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let raw = index as i64 + i64::from(step);
    raw.rem_euclid(len as i64) as usize
}

fn next_available_engine(
    current: usize,
    statuses_len: usize,
    available: &[usize],
    step: i8,
) -> usize {
    if available.is_empty() || statuses_len == 0 {
        return current;
    }
    let mut candidate = current;
    for _ in 0..statuses_len {
        candidate = cycle(candidate, statuses_len, step);
        if available.contains(&candidate) {
            return candidate;
        }
    }
    current
}

fn confirm_config(app: &mut App) -> Option<Command> {
    let Some(Overlay::Config(form)) = app.overlay.clone() else {
        return None;
    };
    let statuses = app.statuses.clone();
    form.apply_to(&mut app.config, &statuses);
    app.overlay = None;
    Some(Command::SaveConfig)
}

fn apply_search_event(app: &mut App, event: SearchEvent) -> Option<Command> {
    match event {
        SearchEvent::PlanReady(plan) => {
            app.progress = vec![SubQueryState::Pending; plan.sub_queries.len()];
            app.plan = Some(plan);
        }
        SearchEvent::SubQueryStarted { idx } => set_progress(app, idx, SubQueryState::Running),
        SearchEvent::SubQueryFinished { idx, ok } => {
            let state = if ok {
                SubQueryState::Done
            } else {
                SubQueryState::Failed
            };
            set_progress(app, idx, state);
        }
        SearchEvent::SynthesisStarted => app.synthesizing = true,
        SearchEvent::AnswerReady(answer) => {
            app.answer = Some(*answer);
            app.screen = Screen::Results;
            app.scroll = 0;
            app.sources_focused = false;
            app.selected_source = 0;
            app.synthesizing = false;
        }
        SearchEvent::LinkChecked { source_id, status } => {
            app.links.insert(source_id, status);
        }
        SearchEvent::Completed => app.synthesizing = false,
        SearchEvent::Failed(message) => {
            app.notice = Some(message);
            app.screen = Screen::Home;
            app.synthesizing = false;
        }
    }
    None
}

fn set_progress(app: &mut App, idx: usize, state: SubQueryState) {
    if let Some(slot) = app.progress.get_mut(idx) {
        *slot = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{Answer, Source};
    use crate::core::config::Config;
    use crate::core::mode::Mode;
    use crate::core::plan::{SearchPlan, SubQuery};
    use crate::engines::{ENGINES, EngineStatus};
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::path::PathBuf;

    fn statuses() -> Vec<EngineStatus> {
        ENGINES
            .iter()
            .enumerate()
            .map(|(index, spec)| EngineStatus {
                spec,
                available: index < 2,
                path: (index < 2).then(|| PathBuf::from("/fake/bin")),
            })
            .collect()
    }

    fn app() -> App {
        App::new(Config::default(), statuses(), None)
    }

    fn key(code: KeyCode) -> AppEvent {
        AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn sample_plan() -> SearchPlan {
        SearchPlan {
            original: "q".to_string(),
            mode: Mode::General,
            answer_lang: "en".to_string(),
            sub_queries: vec![
                SubQuery {
                    query: "a".to_string(),
                    lang: "en".to_string(),
                    rationale: String::new(),
                },
                SubQuery {
                    query: "b".to_string(),
                    lang: "es".to_string(),
                    rationale: String::new(),
                },
            ],
        }
    }

    fn sample_answer() -> Answer {
        Answer {
            title: "t".to_string(),
            sources: vec![
                Source {
                    id: 1,
                    title: "one".to_string(),
                    url: "https://one.example".to_string(),
                    lang: "en".to_string(),
                },
                Source {
                    id: 2,
                    title: "two".to_string(),
                    url: "https://two.example".to_string(),
                    lang: "en".to_string(),
                },
            ],
            ..Answer::default()
        }
    }

    #[test]
    fn typing_on_home_feeds_the_input() {
        let mut app = app();
        update(&mut app, key(KeyCode::Char('r')));
        update(&mut app, key(KeyCode::Char('s')));
        assert_eq!(app.input.value(), "rs");
    }

    #[test]
    fn enter_submits_a_trimmed_query() {
        let mut app = app();
        for letter in "  rust  ".chars() {
            update(&mut app, key(KeyCode::Char(letter)));
        }
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(Command::StartSearch {
                query: "rust".to_string(),
            })
        );
    }

    #[test]
    fn enter_with_empty_input_does_nothing() {
        let mut app = app();
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
    }

    #[test]
    fn tab_cycles_search_modes_in_both_directions() {
        let mut app = app();
        update(&mut app, key(KeyCode::Tab));
        assert_eq!(app.current_mode(), Mode::Scientific);
        update(
            &mut app,
            AppEvent::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        );
        assert_eq!(app.current_mode(), Mode::General);
        update(
            &mut app,
            AppEvent::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
        );
        assert_eq!(app.current_mode(), Mode::Deep);
    }

    #[test]
    fn escape_during_search_cancels_and_returns_home() {
        let mut app = app();
        app.begin_search();
        assert_eq!(app.screen, Screen::Searching);
        let command = update(&mut app, key(KeyCode::Esc));
        assert_eq!(command, Some(Command::CancelSearch));
        assert_eq!(app.screen, Screen::Home);
    }

    #[test]
    fn search_events_drive_progress_and_screen_transitions() {
        let mut app = app();
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::PlanReady(sample_plan())),
        );
        assert_eq!(app.progress, vec![SubQueryState::Pending; 2]);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::SubQueryStarted { idx: 0 }),
        );
        assert_eq!(app.progress[0], SubQueryState::Running);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::SubQueryFinished { idx: 0, ok: true }),
        );
        assert_eq!(app.progress[0], SubQueryState::Done);
        update(&mut app, AppEvent::Search(SearchEvent::SynthesisStarted));
        assert!(app.synthesizing);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(sample_answer()))),
        );
        assert_eq!(app.screen, Screen::Results);
        assert!(!app.synthesizing);
    }

    #[test]
    fn search_failure_returns_home_with_a_notice() {
        let mut app = app();
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::Failed("boom".to_string())),
        );
        assert_eq!(app.screen, Screen::Home);
        assert_eq!(app.notice.as_deref(), Some("boom"));
    }

    #[test]
    fn f1_toggles_help_and_escape_closes_it() {
        let mut app = app();
        update(&mut app, key(KeyCode::F(1)));
        assert_eq!(app.overlay, Some(Overlay::Help));
        update(&mut app, key(KeyCode::Esc));
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn results_keys_scroll_and_open_sources() {
        let mut app = app();
        app.answer = Some(sample_answer());
        app.screen = Screen::Results;
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.scroll, 1);
        update(&mut app, key(KeyCode::Tab));
        assert!(app.sources_focused);
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.selected_source, 1);
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.selected_source, 1);
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(Command::OpenUrl("https://two.example".to_string()))
        );
    }

    #[test]
    fn config_modal_edits_and_saves_settings() {
        let mut app = app();
        update(&mut app, key(KeyCode::F(2)));
        assert!(matches!(app.overlay, Some(Overlay::Config(_))));
        update(&mut app, key(KeyCode::Right));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Right));
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(command, Some(Command::SaveConfig));
        assert_eq!(app.overlay, None);
        assert_eq!(app.config.language, "es");
        assert!(!app.config.validate_links);
    }

    #[test]
    fn config_modal_engine_cycling_skips_unavailable_engines() {
        let mut app = app();
        app.overlay = Some(Overlay::Config(ConfigForm::from_state(
            &app.config,
            &app.statuses,
        )));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Right));
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay open");
        };
        assert_eq!(form.engine_idx, 1);
        let mut app2 = app;
        update(&mut app2, key(KeyCode::Right));
        let Some(Overlay::Config(form)) = &app2.overlay else {
            panic!("config overlay open");
        };
        assert_eq!(form.engine_idx, 0);
    }

    #[test]
    fn quit_commands_come_from_ctrl_c_anywhere_and_q_on_results() {
        let mut app = app();
        let ctrl_c = AppEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(update(&mut app, ctrl_c), Some(Command::Quit));
        app.screen = Screen::Results;
        assert_eq!(
            update(&mut app, key(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
    }
}
