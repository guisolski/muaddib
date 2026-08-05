use crate::core::export::{ExportContext, suggested_filename, to_markdown};
use crate::core::mode::MODES;
use crate::core::tree::{NodeId, NodeSeed};
use crate::pipeline::SearchEvent;
use crate::tui::app::{
    App, ConfigField, ConfigForm, Focus, FollowUpForm, LANGUAGES, Overlay, Screen, Viewport,
    configured_model_idx, model_choices,
};
use crate::tui::event::{AppEvent, Command};
use crate::tui::keymap::{Action, Scope, resolve};
use crate::tui::search_state::{Pulse, SearchOutcome};
use crate::tui::view::doc::{self, DocAnim, DocSelection};
use crossterm::event::KeyEvent;

const PAGE_JUMP: u16 = 10;

pub fn update(app: &mut App, event: AppEvent) -> Option<Command> {
    match event {
        AppEvent::Tick => {
            app.tick = app.tick.wrapping_add(1);
            None
        }
        AppEvent::Resize { width, height } => {
            app.viewport = Viewport { width, height };
            clamp_scroll(app);
            None
        }
        AppEvent::Search(search_event) => apply_search_event(app, search_event),
        AppEvent::Key(key) => handle_key(app, &key),
    }
}

fn scope_of(app: &App) -> Scope {
    match &app.overlay {
        Some(Overlay::FollowUp(_)) => return Scope::FollowUp,
        Some(_) => return Scope::Modal,
        None => {}
    }
    match app.screen {
        Screen::Home => Scope::Home,
        Screen::Searching => Scope::Searching,
        Screen::Results => Scope::Results,
        Screen::Tree => Scope::Tree,
    }
}

fn handle_key(app: &mut App, key: &KeyEvent) -> Option<Command> {
    let scope = scope_of(app);
    let Some(action) = resolve(scope, key) else {
        forward_key_to_input(app, scope, key);
        disarm_clear_history(app);
        return None;
    };
    if action != Action::ClearHistory {
        disarm_clear_history(app);
    }
    app.notice = None;
    perform(app, action)
}

fn forward_key_to_input(app: &mut App, scope: Scope, key: &KeyEvent) {
    use tui_input::backend::crossterm::EventHandler;
    match scope {
        Scope::Home => {
            if app
                .input
                .handle_event(&crossterm::event::Event::Key(*key))
                .is_some()
            {
                app.history_idx = None;
            }
        }
        Scope::FollowUp => {
            if let Some(Overlay::FollowUp(form)) = app.overlay.as_mut() {
                form.input.handle_event(&crossterm::event::Event::Key(*key));
            }
        }
        _ => {}
    }
}

fn disarm_clear_history(app: &mut App) {
    if app.clear_history_armed {
        app.clear_history_armed = false;
        app.notice = None;
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
        Action::ScrollDown
        | Action::ScrollUp
        | Action::PageDown
        | Action::PageUp
        | Action::ScrollTop
        | Action::ScrollBottom => {
            scroll_action(app, action);
            None
        }
        Action::FocusNext => {
            cycle_focus(app, true);
            None
        }
        Action::FocusPrev => {
            cycle_focus(app, false);
            None
        }
        Action::Activate => activate(app),
        Action::JumpToSource(number) => {
            jump_to_source(app, number);
            None
        }
        Action::NewSearch => {
            app.screen = Screen::Home;
            app.input.reset();
            app.search.answer = None;
            None
        }
        Action::RefineSearch => {
            app.screen = Screen::Home;
            None
        }
        Action::HistoryPrev => {
            recall_older(app);
            None
        }
        Action::HistoryNext => {
            recall_newer(app);
            None
        }
        Action::ClearHistory => request_history_clear(app),
        Action::ToggleFast => {
            app.fast = !app.fast;
            None
        }
        Action::OpenTree => {
            open_tree(app);
            None
        }
        Action::OpenFollowUp => {
            open_follow_up(app);
            None
        }
        Action::SubmitFollowUp => submit_follow_up(app),
        Action::SaveSession => save_session(app),
        Action::CopyAnswer => copy_answer(app),
        Action::ExportAnswer => export_answer(app),
        Action::FieldNext | Action::FieldPrev | Action::ValueNext | Action::ValuePrev => {
            if matches!(app.overlay, Some(Overlay::Help)) {
                scroll_help(app, action);
            } else {
                edit_config_form(app, action);
            }
            None
        }
        Action::Confirm => confirm_config(app),
    }
}

fn scroll_action(app: &mut App, action: Action) {
    match action {
        Action::ScrollDown => move_down(app),
        Action::ScrollUp => move_up(app),
        Action::PageDown => {
            app.scroll = app.scroll.saturating_add(PAGE_JUMP).min(max_scroll(app));
        }
        Action::PageUp => app.scroll = app.scroll.saturating_sub(PAGE_JUMP),
        Action::ScrollTop => app.scroll = 0,
        Action::ScrollBottom => app.scroll = max_scroll(app),
        _ => {}
    }
}

fn toggle_help(app: &mut App) {
    app.help_scroll = 0;
    app.overlay = match app.overlay {
        Some(Overlay::Help) => None,
        _ => Some(Overlay::Help),
    };
}

fn scroll_help(app: &mut App, action: Action) {
    let max = crate::tui::view::help::max_scroll(app.viewport.height);
    app.help_scroll = match action {
        Action::FieldNext => app.help_scroll.saturating_add(1).min(max),
        Action::FieldPrev => app.help_scroll.saturating_sub(1),
        _ => app.help_scroll,
    };
}

fn go_back(app: &mut App) -> Option<Command> {
    if app.overlay.is_some() {
        app.overlay = None;
        return None;
    }
    match app.screen {
        Screen::Home => {
            back_from_home(app);
            None
        }
        Screen::Searching => {
            app.screen = Screen::Home;
            app.search.synthesizing = false;
            Some(Command::CancelSearch)
        }
        Screen::Results => {
            app.screen = Screen::Home;
            None
        }
        Screen::Tree => {
            app.screen = if app.search.answer.is_some() {
                Screen::Results
            } else {
                Screen::Home
            };
            None
        }
    }
}

fn back_from_home(app: &mut App) {
    if app.search.answer.is_some() {
        app.screen = Screen::Results;
    } else {
        app.input.reset();
    }
}

fn recall_older(app: &mut App) {
    let Some(last) = app.history.len().checked_sub(1) else {
        return;
    };
    let index = match app.history_idx {
        None => {
            app.history_draft = app.input.value().to_string();
            0
        }
        Some(current) => (current + 1).min(last),
    };
    app.history_idx = Some(index);
    app.input = tui_input::Input::new(app.history[index].clone());
}

fn recall_newer(app: &mut App) {
    let Some(current) = app.history_idx else {
        return;
    };
    if let Some(index) = current.checked_sub(1) {
        app.history_idx = Some(index);
        app.input = tui_input::Input::new(app.history[index].clone());
    } else {
        app.history_idx = None;
        app.input = tui_input::Input::new(app.history_draft.clone());
    }
}

fn request_history_clear(app: &mut App) -> Option<Command> {
    if app.history.is_empty() {
        app.notice = Some("no search history".to_string());
        return None;
    }
    if app.clear_history_armed {
        app.clear_history_armed = false;
        app.notice = None;
        return Some(Command::ClearHistory);
    }
    app.clear_history_armed = true;
    app.notice = Some(format!(
        "press Ctrl+L again to clear {} searches",
        app.history.len()
    ));
    None
}

fn submit_query(app: &mut App) -> Option<Command> {
    let query = app.input.value().trim().to_string();
    if query.is_empty() {
        return None;
    }
    app.pending_parent = None;
    Some(Command::StartSearch { query })
}

fn open_tree(app: &mut App) {
    if app.tree.nodes.is_empty() {
        app.notice = Some("no research yet".to_string());
        return;
    }
    app.tree_sel = app
        .tree
        .current
        .and_then(|current| app.tree.flatten().iter().position(|row| row.id == current))
        .unwrap_or(0);
    app.screen = Screen::Tree;
}

fn open_follow_up(app: &mut App) {
    let parent = if app.screen == Screen::Tree {
        selected_tree_node(app)
    } else {
        app.tree.current
    };
    let Some(parent) = parent else {
        app.notice = Some("no research to branch from".to_string());
        return;
    };
    app.overlay = Some(Overlay::FollowUp(FollowUpForm {
        input: tui_input::Input::default(),
        parent,
    }));
}

fn selected_tree_node(app: &App) -> Option<NodeId> {
    app.tree.flatten().get(app.tree_sel).map(|row| row.id)
}

fn submit_follow_up(app: &mut App) -> Option<Command> {
    let Some(Overlay::FollowUp(form)) = &app.overlay else {
        return None;
    };
    let query = form.input.value().trim().to_string();
    if query.is_empty() {
        return None;
    }
    app.pending_parent = Some(form.parent);
    app.overlay = None;
    app.input = tui_input::Input::new(query.clone());
    Some(Command::StartSearch { query })
}

fn save_session(app: &mut App) -> Option<Command> {
    if app.tree.nodes.is_empty() {
        app.notice = Some("no research to save".to_string());
        return None;
    }
    Some(Command::SaveSession)
}

fn export_context(app: &App) -> ExportContext {
    let (query, mode) = app.search.plan.as_ref().map_or_else(
        || (app.input.value().trim().to_string(), app.current_mode()),
        |plan| (plan.original.clone(), plan.mode),
    );
    ExportContext { query, mode }
}

fn rendered_answer(app: &App) -> Option<(String, String)> {
    let answer = app.search.answer.as_ref()?;
    Some((
        suggested_filename(answer),
        to_markdown(answer, &export_context(app)),
    ))
}

fn copy_answer(app: &mut App) -> Option<Command> {
    let Some((_, markdown)) = rendered_answer(app) else {
        app.notice = Some("no answer to copy".to_string());
        return None;
    };
    Some(Command::CopyAnswer(markdown))
}

fn export_answer(app: &mut App) -> Option<Command> {
    let Some((filename, contents)) = rendered_answer(app) else {
        app.notice = Some("no answer to export".to_string());
        return None;
    };
    Some(Command::ExportAnswer { filename, contents })
}

fn move_down(app: &mut App) {
    if app.screen == Screen::Tree {
        app.tree_sel = step_index(app.tree_sel, true, app.tree.nodes.len());
        return;
    }
    move_focus(app, true);
}

fn move_up(app: &mut App) {
    if app.screen == Screen::Tree {
        app.tree_sel = step_index(app.tree_sel, false, app.tree.nodes.len());
        return;
    }
    move_focus(app, false);
}

fn move_focus(app: &mut App, forward: bool) {
    match app.focus {
        Focus::Body => {
            app.scroll = if forward {
                app.scroll.saturating_add(1).min(max_scroll(app))
            } else {
                app.scroll.saturating_sub(1)
            };
        }
        Focus::Sources(index) => {
            app.focus = Focus::Sources(step_index(index, forward, app.source_count()));
            ensure_selection_visible(app);
        }
        Focus::Followups(index) => {
            app.focus = Focus::Followups(step_index(index, forward, app.followup_count()));
            ensure_selection_visible(app);
        }
    }
}

fn step_index(index: usize, forward: bool, count: usize) -> usize {
    if forward {
        (index + 1).min(count.saturating_sub(1))
    } else {
        index.saturating_sub(1)
    }
}

fn cycle_focus(app: &mut App, forward: bool) {
    let current = match app.focus {
        Focus::Body => 0,
        Focus::Sources(_) => 1,
        Focus::Followups(_) => 2,
    };
    let populated = [true, app.source_count() > 0, app.followup_count() > 0];
    let mut position = current;
    for _ in 0..populated.len() {
        position = if forward {
            (position + 1) % populated.len()
        } else {
            (position + populated.len() - 1) % populated.len()
        };
        if populated[position] {
            break;
        }
    }
    app.focus = match position {
        1 => Focus::Sources(0),
        2 => Focus::Followups(0),
        _ => Focus::Body,
    };
    ensure_selection_visible(app);
}

fn activate(app: &mut App) -> Option<Command> {
    if app.screen == Screen::Tree {
        project_selected_node(app);
        return None;
    }
    let answer = app.search.answer.as_ref()?;
    match app.focus {
        Focus::Body => None,
        Focus::Sources(index) => answer
            .sources
            .get(index)
            .map(|source| Command::OpenUrl(source.url.clone())),
        Focus::Followups(index) => {
            let query = answer.followups.get(index)?.clone();
            app.input = tui_input::Input::new(query.clone());
            app.pending_parent = app.tree.current;
            Some(Command::StartSearch { query })
        }
    }
}

fn project_selected_node(app: &mut App) {
    let Some(id) = selected_tree_node(app) else {
        return;
    };
    let Some(node) = app.tree.node(id) else {
        return;
    };
    let answer = node.answer.clone();
    app.tree.current = Some(id);
    app.search.answer = Some(answer);
    app.search.links.clear();
    app.search.images.clear();
    app.search.reveal_started = None;
    app.search.pulse = None;
    app.image_runtime.clear();
    app.scroll = 0;
    app.focus = Focus::Body;
    app.screen = Screen::Results;
}

fn jump_to_source(app: &mut App, number: u8) {
    let index = usize::from(number).saturating_sub(1);
    if index < app.source_count() {
        app.focus = Focus::Sources(index);
        app.search.pulse = Some(Pulse {
            source: index,
            started: app.tick,
        });
        ensure_selection_visible(app);
    }
}

fn ensure_selection_visible(app: &mut App) {
    let Some(answer) = &app.search.answer else {
        return;
    };
    let width = doc::content_width(app.viewport.width);
    let settled = DocAnim::settled(answer.blocks.len());
    let rendered = doc::render_doc(
        answer,
        width,
        &app.search.links,
        DocSelection::None,
        &settled,
        &app.search.images,
    );
    let range = match app.focus {
        Focus::Body => None,
        Focus::Sources(index) => rendered.source_ranges.get(index).copied(),
        Focus::Followups(index) => rendered.followup_ranges.get(index).copied(),
    };
    if let Some(range) = range {
        app.scroll =
            doc::scroll_into_view(app.scroll, range, doc::content_height(app.viewport.height));
    }
}

fn max_scroll(app: &App) -> u16 {
    let Some(answer) = &app.search.answer else {
        return 0;
    };
    let width = doc::content_width(app.viewport.width);
    let settled = DocAnim::settled(answer.blocks.len());
    let rendered = doc::render_doc(
        answer,
        width,
        &app.search.links,
        DocSelection::None,
        &settled,
        &app.search.images,
    );
    let total = u16::try_from(rendered.lines.len()).unwrap_or(u16::MAX);
    total.saturating_sub(doc::content_height(app.viewport.height))
}

fn clamp_scroll(app: &mut App) {
    app.scroll = app.scroll.min(max_scroll(app));
}

struct FormContext {
    statuses_len: usize,
    available: Vec<usize>,
    model_counts: Vec<usize>,
    configured_model_idxs: Vec<usize>,
}

impl FormContext {
    fn from_app(app: &App) -> Self {
        Self {
            statuses_len: app.statuses.len(),
            available: app
                .statuses
                .iter()
                .enumerate()
                .filter(|(_, status)| status.available)
                .map(|(index, _)| index)
                .collect(),
            model_counts: app
                .statuses
                .iter()
                .map(|status| model_choices(&app.config, status.spec).len())
                .collect(),
            configured_model_idxs: app
                .statuses
                .iter()
                .map(|status| configured_model_idx(&app.config, status.spec))
                .collect(),
        }
    }
}

fn edit_config_form(app: &mut App, action: Action) {
    let context = FormContext::from_app(app);
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
        Action::ValueNext => step_config_value(form, 1, &context),
        Action::ValuePrev => step_config_value(form, -1, &context),
        _ => {}
    }
}

fn step_config_value(form: &mut ConfigForm, step: i8, context: &FormContext) {
    match form.field() {
        ConfigField::Language => {
            form.language_idx = cycle(form.language_idx, LANGUAGES.len(), step);
        }
        ConfigField::Engine => {
            form.engine_idx = next_available_engine(
                form.engine_idx,
                context.statuses_len,
                &context.available,
                step,
            );
            form.model_idx = context
                .configured_model_idxs
                .get(form.engine_idx)
                .copied()
                .unwrap_or(0);
        }
        ConfigField::Model => {
            let count = context
                .model_counts
                .get(form.engine_idx)
                .copied()
                .unwrap_or(1);
            form.model_idx = cycle(form.model_idx, count, step);
        }
        ConfigField::ValidateLinks => form.validate_links = !form.validate_links,
        ConfigField::WebSearch => form.websearch = !form.websearch,
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
    match app.search.apply_event(event, app.tick) {
        SearchOutcome::AnswerReady => {
            capture_node(app);
            app.screen = Screen::Results;
            app.scroll = 0;
            app.focus = Focus::Body;
        }
        SearchOutcome::LinkChecked { source_id, status } => {
            if let Some(current) = app.tree.current {
                app.tree.set_source_status(current, source_id, status);
            }
        }
        SearchOutcome::Failed(message) => {
            app.notice = Some(message);
            app.pending_parent = None;
            app.screen = Screen::Home;
        }
        SearchOutcome::Completed | SearchOutcome::None => {}
    }
    None
}

fn capture_node(app: &mut App) {
    let Some(answer) = app.search.answer.clone() else {
        return;
    };
    let (query, mode, sub_queries) = app.search.plan.as_ref().map_or_else(
        || {
            (
                app.input.value().trim().to_string(),
                app.current_mode(),
                Vec::new(),
            )
        },
        |plan| (plan.original.clone(), plan.mode, plan.sub_queries.clone()),
    );
    let parent = app.pending_parent.take();
    app.tree.add_node(
        parent,
        NodeSeed {
            query,
            mode,
            fast: app.fast,
            started_at: app.search_started_unix,
            completed_at: app.clock_unix,
            answer,
            sub_queries,
            web_urls: app.search.web_urls.clone(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{Answer, Block, Emphasis, Source};
    use crate::core::citations::LinkStatus;
    use crate::core::config::Config;
    use crate::core::mode::Mode;
    use crate::core::plan::{SearchPlan, SubQuery};
    use crate::engines::{ENGINES, EngineStatus};
    use crate::tui::search_state::{ImageFetch, SubQueryState};
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
        App::new(Config::default(), statuses(), None, false)
    }

    fn app_with_history(entries: &[&str]) -> App {
        let mut app = app();
        app.history = entries.iter().map(ToString::to_string).collect();
        app
    }

    fn key(code: KeyCode) -> AppEvent {
        AppEvent::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(letter: char) -> AppEvent {
        AppEvent::Key(KeyEvent::new(KeyCode::Char(letter), KeyModifiers::CONTROL))
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
                    status: None,
                    ..Default::default()
                },
                Source {
                    id: 2,
                    title: "two".to_string(),
                    url: "https://two.example".to_string(),
                    lang: "en".to_string(),
                    status: None,
                    ..Default::default()
                },
            ],
            ..Answer::default()
        }
    }

    fn scroll_answer() -> Answer {
        Answer {
            blocks: vec![Block::Paragraph {
                text: "body".to_string(),
                source_ids: Vec::new(),
                emphasis: Emphasis::None,
            }],
            ..sample_answer()
        }
    }

    fn results_app(answer: Answer, height: u16) -> App {
        let mut app = app();
        app.search.answer = Some(answer);
        app.screen = Screen::Results;
        app.viewport = Viewport { width: 40, height };
        app
    }

    #[test]
    fn copy_and_export_render_the_answer_as_markdown() {
        struct Case {
            name: &'static str,
            key: char,
            want_filename: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "y copies without a filename",
                key: 'y',
                want_filename: None,
            },
            Case {
                name: "e exports to a slugged filename",
                key: 'e',
                want_filename: Some("muaddib-t.md"),
            },
        ];
        for case in cases {
            let mut app = results_app(sample_answer(), 24);
            let command = update(&mut app, key(KeyCode::Char(case.key)));
            let markdown = match (command, case.want_filename) {
                (Some(Command::CopyAnswer(markdown)), None) => markdown,
                (Some(Command::ExportAnswer { filename, contents }), Some(want)) => {
                    assert_eq!(filename, want, "{}", case.name);
                    contents
                }
                (other, _) => panic!("{}: unexpected {other:?}", case.name),
            };
            assert!(markdown.starts_with("# t\n"), "{}", case.name);
            assert!(markdown.contains("https://one.example"), "{}", case.name);
        }
    }

    #[test]
    fn copy_and_export_report_when_there_is_no_answer() {
        for character in ['y', 'e'] {
            let mut app = app();
            app.screen = Screen::Results;
            assert_eq!(update(&mut app, key(KeyCode::Char(character))), None);
            assert!(
                app.notice
                    .as_deref()
                    .is_some_and(|notice| notice.starts_with("no answer to")),
                "{character}: {:?}",
                app.notice
            );
        }
    }

    #[test]
    fn a_checked_link_lands_on_the_answer_and_the_captured_node() {
        let mut app = results_app(sample_answer(), 24);
        app.tree.add_node(
            None,
            NodeSeed {
                query: "q".to_string(),
                mode: app.current_mode(),
                fast: false,
                started_at: 0,
                completed_at: 0,
                answer: sample_answer(),
                sub_queries: Vec::new(),
                web_urls: Vec::new(),
            },
        );
        update(
            &mut app,
            AppEvent::Search(SearchEvent::LinkChecked {
                source_id: 2,
                status: LinkStatus::Invalid(404),
            }),
        );
        let live = &app.search.answer.as_ref().unwrap().sources[1];
        assert_eq!(live.status, Some(LinkStatus::Invalid(404)));
        let stored = &app.tree.current_node().unwrap().answer.sources[1];
        assert_eq!(stored.status, Some(LinkStatus::Invalid(404)));
    }

    #[test]
    fn any_keypress_dismisses_the_last_notice() {
        let mut app = results_app(sample_answer(), 24);
        app.notice = Some("answer copied as markdown".to_string());
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.notice, None);
    }

    #[test]
    fn scroll_actions_clamp_to_document_end() {
        struct Case {
            name: &'static str,
            key: KeyCode,
            mods: KeyModifiers,
            presses: usize,
            start: u16,
            want: u16,
        }
        let cases = [
            Case {
                name: "j stops at the last scrollable line",
                key: KeyCode::Char('j'),
                mods: KeyModifiers::NONE,
                presses: 10,
                start: 0,
                want: 3,
            },
            Case {
                name: "shift g lands exactly on the maximum",
                key: KeyCode::Char('G'),
                mods: KeyModifiers::SHIFT,
                presses: 1,
                start: 0,
                want: 3,
            },
            Case {
                name: "page down clamps to the maximum",
                key: KeyCode::PageDown,
                mods: KeyModifiers::NONE,
                presses: 1,
                start: 0,
                want: 3,
            },
            Case {
                name: "g returns to the top",
                key: KeyCode::Char('g'),
                mods: KeyModifiers::NONE,
                presses: 1,
                start: 3,
                want: 0,
            },
        ];
        for case in cases {
            let mut app = results_app(scroll_answer(), 5);
            app.scroll = case.start;
            for _ in 0..case.presses {
                update(&mut app, AppEvent::Key(KeyEvent::new(case.key, case.mods)));
            }
            assert_eq!(app.scroll, case.want, "{}", case.name);
        }
    }

    #[test]
    fn resize_updates_viewport_and_reclamps_scroll() {
        struct Case {
            name: &'static str,
            start_height: u16,
            start_scroll: u16,
            new_height: u16,
            want_scroll: u16,
        }
        let cases = [
            Case {
                name: "shrinking keeps a valid scroll",
                start_height: 6,
                start_scroll: 2,
                new_height: 5,
                want_scroll: 2,
            },
            Case {
                name: "growing clamps scroll to the new maximum",
                start_height: 5,
                start_scroll: 3,
                new_height: 6,
                want_scroll: 2,
            },
        ];
        for case in cases {
            let mut app = results_app(scroll_answer(), case.start_height);
            app.scroll = case.start_scroll;
            update(
                &mut app,
                AppEvent::Resize {
                    width: 40,
                    height: case.new_height,
                },
            );
            assert_eq!(
                app.viewport,
                Viewport {
                    width: 40,
                    height: case.new_height,
                },
                "{}",
                case.name
            );
            assert_eq!(app.scroll, case.want_scroll, "{}", case.name);
        }
    }

    fn interactive_answer() -> Answer {
        Answer {
            followups: vec!["next".to_string()],
            ..sample_answer()
        }
    }

    #[test]
    fn tab_cycles_focus_through_body_sources_and_followups() {
        struct Case {
            name: &'static str,
            answer: Answer,
            forward: bool,
            want: Vec<Focus>,
        }
        let cases = [
            Case {
                name: "full answer cycles all three panes",
                answer: interactive_answer(),
                forward: true,
                want: vec![Focus::Sources(0), Focus::Followups(0), Focus::Body],
            },
            Case {
                name: "no followups skips back to body",
                answer: sample_answer(),
                forward: true,
                want: vec![Focus::Sources(0), Focus::Body],
            },
            Case {
                name: "no sources skips to followups",
                answer: Answer {
                    sources: Vec::new(),
                    ..interactive_answer()
                },
                forward: true,
                want: vec![Focus::Followups(0), Focus::Body],
            },
            Case {
                name: "backtab reverses the cycle",
                answer: interactive_answer(),
                forward: false,
                want: vec![Focus::Followups(0), Focus::Sources(0), Focus::Body],
            },
        ];
        for case in cases {
            let mut app = results_app(case.answer, 24);
            for (step, want) in case.want.iter().enumerate() {
                let event = if case.forward {
                    key(KeyCode::Tab)
                } else {
                    AppEvent::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
                };
                update(&mut app, event);
                assert_eq!(app.focus, *want, "{} step {step}", case.name);
            }
        }
    }

    #[test]
    fn jk_moves_selection_within_pane_and_scrolls_it_into_view() {
        let mut app = results_app(interactive_answer(), 5);
        update(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Sources(0));
        assert_eq!(app.scroll, 0);
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.focus, Focus::Sources(1));
        assert_eq!(app.scroll, 1);
        update(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Followups(0));
        assert_eq!(app.scroll, 3);
    }

    #[test]
    fn enter_activates_selection_per_focus() {
        struct Case {
            name: &'static str,
            focus: Focus,
            want: Option<Command>,
            want_input: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "body enter does nothing",
                focus: Focus::Body,
                want: None,
                want_input: None,
            },
            Case {
                name: "sources enter opens the selected url",
                focus: Focus::Sources(1),
                want: Some(Command::OpenUrl("https://two.example".to_string())),
                want_input: None,
            },
            Case {
                name: "followups enter runs it as a new search",
                focus: Focus::Followups(0),
                want: Some(Command::StartSearch {
                    query: "next".to_string(),
                }),
                want_input: Some("next"),
            },
        ];
        for case in cases {
            let mut app = results_app(interactive_answer(), 24);
            app.focus = case.focus;
            let command = update(&mut app, key(KeyCode::Enter));
            assert_eq!(command, case.want, "{}", case.name);
            if let Some(want_input) = case.want_input {
                assert_eq!(app.input.value(), want_input, "{}", case.name);
            }
        }
    }

    #[test]
    fn digit_keys_jump_to_existing_sources_only() {
        struct Case {
            name: &'static str,
            digit: char,
            want_focus: Focus,
            want_scroll: u16,
        }
        let cases = [
            Case {
                name: "digit two selects the second source and scrolls to it",
                digit: '2',
                want_focus: Focus::Sources(1),
                want_scroll: 1,
            },
            Case {
                name: "digit nine with two sources changes nothing",
                digit: '9',
                want_focus: Focus::Body,
                want_scroll: 0,
            },
        ];
        for case in cases {
            let mut app = results_app(interactive_answer(), 5);
            update(&mut app, key(KeyCode::Char(case.digit)));
            assert_eq!(app.focus, case.want_focus, "{}", case.name);
            assert_eq!(app.scroll, case.want_scroll, "{}", case.name);
        }
    }

    #[test]
    fn answer_ready_starts_reveal_and_digit_jump_starts_pulse() {
        let mut app = app();
        app.begin_search();
        app.tick = 42;
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(interactive_answer()))),
        );
        assert_eq!(app.search.reveal_started, Some(42));
        app.viewport = Viewport {
            width: 40,
            height: 24,
        };
        app.tick = 50;
        update(&mut app, key(KeyCode::Char('2')));
        assert_eq!(
            app.search.pulse,
            Some(Pulse {
                source: 1,
                started: 50,
            })
        );
        app.begin_search();
        assert_eq!(app.search.reveal_started, None);
        assert_eq!(app.search.pulse, None);
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
        assert_eq!(app.current_mode(), Mode::Exhaustive);
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
        assert_eq!(app.search.progress, vec![SubQueryState::Pending; 2]);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::SubQueryStarted { idx: 0 }),
        );
        assert_eq!(app.search.progress[0], SubQueryState::Running);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::SubQueryFinished { idx: 0, ok: true }),
        );
        assert_eq!(app.search.progress[0], SubQueryState::Done);
        update(&mut app, AppEvent::Search(SearchEvent::SynthesisStarted));
        assert!(app.search.synthesizing);
        app.focus = Focus::Sources(1);
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(sample_answer()))),
        );
        assert_eq!(app.screen, Screen::Results);
        assert_eq!(app.focus, Focus::Body);
        assert!(!app.search.synthesizing);
    }

    #[test]
    fn image_fetch_events_store_bytes_or_failures_until_the_next_search() {
        let mut app = app();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::ImageFetched {
                url: "https://img.example/a.png".to_string(),
                bytes: Some(vec![1, 2, 3]),
            }),
        );
        update(
            &mut app,
            AppEvent::Search(SearchEvent::ImageFetched {
                url: "https://img.example/b.png".to_string(),
                bytes: None,
            }),
        );
        assert_eq!(
            app.search.images.get("https://img.example/a.png"),
            Some(&ImageFetch::Ready(vec![1, 2, 3]))
        );
        assert_eq!(
            app.search.images.get("https://img.example/b.png"),
            Some(&ImageFetch::Failed)
        );
        app.begin_search();
        assert!(app.search.images.is_empty());
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
    fn ctrl_g_toggles_help_and_escape_closes_it() {
        let mut app = app();
        update(&mut app, ctrl('g'));
        assert_eq!(app.overlay, Some(Overlay::Help));
        update(&mut app, key(KeyCode::Esc));
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn arrow_keys_scroll_the_help_modal_within_bounds() {
        let mut app = app();
        app.viewport = Viewport {
            width: 80,
            height: 24,
        };
        update(&mut app, ctrl('g'));
        assert_eq!(app.help_scroll, 0);
        update(&mut app, key(KeyCode::Down));
        assert_eq!(app.help_scroll, 1);
        update(&mut app, key(KeyCode::Up));
        update(&mut app, key(KeyCode::Up));
        assert_eq!(app.help_scroll, 0);
        let max = crate::tui::view::help::max_scroll(24);
        for _ in 0..200 {
            update(&mut app, key(KeyCode::Down));
        }
        assert_eq!(app.help_scroll, max);
        update(&mut app, ctrl('g'));
        update(&mut app, ctrl('g'));
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn refine_search_can_return_to_the_previous_results() {
        let mut app = app();
        app.search.answer = Some(sample_answer());
        app.screen = Screen::Results;
        update(&mut app, key(KeyCode::Char('/')));
        assert_eq!(app.screen, Screen::Home);
        update(&mut app, key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Results);
        assert!(app.search.answer.is_some());
    }

    #[test]
    fn escape_on_home_without_results_clears_the_input() {
        let mut app = app();
        update(&mut app, key(KeyCode::Char('x')));
        update(&mut app, key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Home);
        assert_eq!(app.input.value(), "");
    }

    #[test]
    fn results_keys_scroll_and_open_sources() {
        let mut app = results_app(sample_answer(), 4);
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.scroll, 1);
        update(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Sources(0));
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.focus, Focus::Sources(1));
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.focus, Focus::Sources(1));
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(Command::OpenUrl("https://two.example".to_string()))
        );
    }

    #[test]
    fn config_modal_edits_and_saves_settings() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        assert!(matches!(app.overlay, Some(Overlay::Config(_))));
        update(&mut app, key(KeyCode::Right));
        update(&mut app, key(KeyCode::Down));
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
    fn config_modal_toggles_web_search_grounding() {
        let mut app = app();
        assert!(app.config.websearch.enabled);
        update(&mut app, ctrl('o'));
        for _ in 0..4 {
            update(&mut app, key(KeyCode::Down));
        }
        update(&mut app, key(KeyCode::Right));
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(command, Some(Command::SaveConfig));
        assert!(!app.config.websearch.enabled);
        update(&mut app, ctrl('o'));
        for _ in 0..4 {
            update(&mut app, key(KeyCode::Down));
        }
        update(&mut app, key(KeyCode::Left));
        update(&mut app, key(KeyCode::Enter));
        assert!(app.config.websearch.enabled);
    }

    #[test]
    fn config_modal_saves_a_model_override_for_the_selected_engine() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Right));
        update(&mut app, key(KeyCode::Enter));
        assert_eq!(app.config.model_override("claude"), Some("opus"));
        update(&mut app, ctrl('o'));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Left));
        update(&mut app, key(KeyCode::Enter));
        assert_eq!(app.config.model_override("claude"), None);
    }

    #[test]
    fn cycling_the_engine_resets_the_model_to_that_engines_configured_one() {
        let mut app = app();
        app.config
            .set_model_override("claude", Some("sonnet".to_string()));
        update(&mut app, ctrl('o'));
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay open");
        };
        assert_eq!(form.model_idx, 2);
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Right));
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay open");
        };
        assert_eq!(form.engine_idx, 1);
        assert_eq!(form.model_idx, 0);
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

    fn type_query(app: &mut App, text: &str) {
        for character in text.chars() {
            update(app, key(KeyCode::Char(character)));
        }
    }

    #[test]
    fn arrow_keys_walk_the_search_history_newest_first() {
        struct Case {
            name: &'static str,
            presses: &'static [KeyCode],
            want: &'static str,
        }
        let cases = [
            Case {
                name: "one up recalls the newest",
                presses: &[KeyCode::Up],
                want: "gamma",
            },
            Case {
                name: "two ups reach the second entry",
                presses: &[KeyCode::Up, KeyCode::Up],
                want: "beta",
            },
            Case {
                name: "up stops at the oldest entry",
                presses: &[KeyCode::Up, KeyCode::Up, KeyCode::Up, KeyCode::Up],
                want: "alpha",
            },
            Case {
                name: "down walks back toward the newest",
                presses: &[KeyCode::Up, KeyCode::Up, KeyCode::Down],
                want: "gamma",
            },
            Case {
                name: "down alone does nothing",
                presses: &[KeyCode::Down],
                want: "",
            },
        ];
        for case in cases {
            let mut app = app_with_history(&["gamma", "beta", "alpha"]);
            for press in case.presses {
                update(&mut app, key(*press));
            }
            assert_eq!(app.input.value(), case.want, "{}", case.name);
        }
    }

    #[test]
    fn stepping_past_the_newest_entry_restores_the_draft() {
        let mut app = app_with_history(&["gamma", "beta"]);
        type_query(&mut app, "half typed");
        update(&mut app, key(KeyCode::Up));
        assert_eq!(app.input.value(), "gamma");
        update(&mut app, key(KeyCode::Down));
        assert_eq!(app.input.value(), "half typed");
        assert_eq!(app.history_idx, None);
    }

    #[test]
    fn typing_after_a_recall_leaves_history_navigation() {
        let mut app = app_with_history(&["gamma", "beta"]);
        update(&mut app, key(KeyCode::Up));
        assert_eq!(app.history_idx, Some(0));
        type_query(&mut app, "!");
        assert_eq!(app.history_idx, None);
        assert_eq!(app.input.value(), "gamma!");
        update(&mut app, key(KeyCode::Up));
        assert_eq!(app.input.value(), "gamma");
        update(&mut app, key(KeyCode::Down));
        assert_eq!(app.input.value(), "gamma!");
    }

    #[test]
    fn history_recall_is_inert_when_nothing_was_searched() {
        let mut app = app();
        type_query(&mut app, "fresh");
        update(&mut app, key(KeyCode::Up));
        assert_eq!(app.input.value(), "fresh");
        assert_eq!(app.history_idx, None);
    }

    #[test]
    fn clearing_history_takes_two_presses() {
        let mut app = app_with_history(&["gamma", "beta"]);
        assert_eq!(update(&mut app, ctrl('l')), None);
        assert!(app.clear_history_armed);
        assert_eq!(
            app.notice.as_deref(),
            Some("press Ctrl+L again to clear 2 searches")
        );
        assert_eq!(update(&mut app, ctrl('l')), Some(Command::ClearHistory));
        assert!(!app.clear_history_armed);
        assert_eq!(app.notice, None);
    }

    #[test]
    fn any_other_key_disarms_the_history_clear() {
        struct Case {
            name: &'static str,
            interruption: KeyCode,
        }
        let cases = [
            Case {
                name: "typing a character",
                interruption: KeyCode::Char('x'),
            },
            Case {
                name: "recalling history",
                interruption: KeyCode::Up,
            },
            Case {
                name: "cycling the mode",
                interruption: KeyCode::Tab,
            },
        ];
        for case in cases {
            let mut app = app_with_history(&["gamma"]);
            update(&mut app, ctrl('l'));
            update(&mut app, key(case.interruption));
            assert!(!app.clear_history_armed, "{}", case.name);
            assert_eq!(app.notice, None, "{}", case.name);
            assert_eq!(update(&mut app, ctrl('l')), None, "{}", case.name);
        }
    }

    #[test]
    fn clearing_an_empty_history_reports_instead_of_arming() {
        let mut app = app();
        assert_eq!(update(&mut app, ctrl('l')), None);
        assert!(!app.clear_history_armed);
        assert_eq!(app.notice.as_deref(), Some("no search history"));
    }

    fn searched_app() -> App {
        let mut app = app();
        app.input = tui_input::Input::new("q".to_string());
        app.clock_unix = 100;
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::PlanReady(sample_plan())),
        );
        update(
            &mut app,
            AppEvent::Search(SearchEvent::WebHits {
                count: 1,
                urls: vec!["https://hit.example".to_string()],
            }),
        );
        app.clock_unix = 130;
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(interactive_answer()))),
        );
        app.viewport = Viewport {
            width: 40,
            height: 24,
        };
        app
    }

    #[test]
    fn answer_ready_captures_a_research_node() {
        let app = searched_app();
        assert_eq!(app.tree.nodes.len(), 1);
        let node = app.tree.current_node().unwrap();
        assert_eq!(node.query, "q");
        assert_eq!(node.parent, None);
        assert_eq!(node.sub_queries.len(), 2);
        assert_eq!(node.web_urls, vec!["https://hit.example"]);
        assert_eq!(node.started_at, 100);
        assert_eq!(node.completed_at, 130);
        assert_eq!(app.screen, Screen::Results);
    }

    #[test]
    fn follow_up_overlay_types_submits_and_branches_from_the_current_node() {
        let mut app = searched_app();
        let root = app.tree.current.unwrap();
        update(&mut app, key(KeyCode::Char('f')));
        assert!(matches!(app.overlay, Some(Overlay::FollowUp(_))));
        type_query(&mut app, "why though");
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(Command::StartSearch {
                query: "why though".to_string(),
            })
        );
        assert_eq!(app.overlay, None);
        assert_eq!(app.pending_parent, Some(root));
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(sample_answer()))),
        );
        assert_eq!(app.tree.nodes.len(), 2);
        assert_eq!(app.tree.nodes[1].parent, Some(root));
        assert_eq!(app.tree.current, Some(app.tree.nodes[1].id));
    }

    #[test]
    fn empty_follow_up_submit_keeps_the_overlay_open() {
        let mut app = searched_app();
        update(&mut app, key(KeyCode::Char('f')));
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        assert!(matches!(app.overlay, Some(Overlay::FollowUp(_))));
        assert_eq!(app.pending_parent, None);
    }

    #[test]
    fn follow_up_without_research_reports_instead_of_opening() {
        let mut app = results_app(interactive_answer(), 24);
        update(&mut app, key(KeyCode::Char('f')));
        assert_eq!(app.overlay, None);
        assert_eq!(app.notice.as_deref(), Some("no research to branch from"));
    }

    #[test]
    fn followup_suggestion_enter_branches_from_the_current_node() {
        let mut app = searched_app();
        let root = app.tree.current.unwrap();
        app.focus = Focus::Followups(0);
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            command,
            Some(Command::StartSearch {
                query: "next".to_string(),
            })
        );
        assert_eq!(app.pending_parent, Some(root));
    }

    #[test]
    fn tree_screen_navigates_projects_and_branches() {
        let mut app = searched_app();
        let root = app.tree.current.unwrap();
        app.pending_parent = Some(root);
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(sample_answer()))),
        );
        update(&mut app, key(KeyCode::Char('t')));
        assert_eq!(app.screen, Screen::Tree);
        assert_eq!(app.tree_sel, 1);
        update(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.tree_sel, 0);
        update(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.tree_sel, 0);
        update(&mut app, key(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Results);
        assert_eq!(app.tree.current, Some(root));
        assert_eq!(
            app.search
                .answer
                .as_ref()
                .map(|answer| answer.title.as_str()),
            Some("t")
        );
        update(&mut app, key(KeyCode::Char('t')));
        update(&mut app, key(KeyCode::Char('j')));
        update(&mut app, key(KeyCode::Char('f')));
        assert!(
            matches!(&app.overlay, Some(Overlay::FollowUp(form)) if form.parent == app.tree.nodes[1].id)
        );
    }

    #[test]
    fn open_tree_without_research_reports_a_notice() {
        let mut app = results_app(sample_answer(), 24);
        update(&mut app, key(KeyCode::Char('t')));
        assert_eq!(app.screen, Screen::Results);
        assert_eq!(app.notice.as_deref(), Some("no research yet"));
    }

    #[test]
    fn new_search_wipes_the_answer_but_keeps_the_tree() {
        let mut app = searched_app();
        update(&mut app, key(KeyCode::Char('n')));
        assert_eq!(app.screen, Screen::Home);
        assert!(app.search.answer.is_none());
        assert_eq!(app.tree.nodes.len(), 1);
    }

    #[test]
    fn save_session_needs_research_to_save() {
        let mut app = results_app(sample_answer(), 24);
        assert_eq!(update(&mut app, key(KeyCode::Char('s'))), None);
        assert_eq!(app.notice.as_deref(), Some("no research to save"));
        let mut searched = searched_app();
        assert_eq!(
            update(&mut searched, key(KeyCode::Char('s'))),
            Some(Command::SaveSession)
        );
    }

    #[test]
    fn home_submit_starts_a_new_research_root() {
        let mut app = searched_app();
        update(&mut app, key(KeyCode::Char('/')));
        assert_eq!(app.screen, Screen::Home);
        let command = update(&mut app, key(KeyCode::Enter));
        assert!(matches!(command, Some(Command::StartSearch { .. })));
        assert_eq!(app.pending_parent, None);
        app.begin_search();
        update(
            &mut app,
            AppEvent::Search(SearchEvent::AnswerReady(Box::new(sample_answer()))),
        );
        assert_eq!(app.tree.nodes[1].parent, None);
    }

    #[test]
    fn ctrl_f_toggles_fast_mode_from_home_and_results() {
        let mut app = app();
        assert!(!app.fast);
        update(&mut app, ctrl('f'));
        assert!(app.fast);
        app.screen = Screen::Results;
        update(&mut app, ctrl('f'));
        assert!(!app.fast);
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
