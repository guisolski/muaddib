use crate::core::engine::EngineSpec;
use crate::core::export::{ExportContext, suggested_filename, to_markdown};
use crate::core::mode::MODES;
use crate::core::tree::{NodeId, NodeSeed};
use crate::core::vault::Passphrase;
use crate::pipeline::SearchEvent;
use crate::tui::app::{
    App, ConfigField, ConfigForm, Focus, FollowUpForm, LANGUAGES, Overlay, PassphraseForm, Screen,
    Viewport, configured_model_idx, model_choices, visible_config_fields,
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
        Some(Overlay::Passphrase(_)) => return Scope::Entry,
        Some(Overlay::Config(form)) if form.editing_key || form.editing_url => {
            return Scope::Entry;
        }
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
        Scope::Entry => match app.overlay.as_mut() {
            Some(Overlay::Passphrase(form)) => {
                form.error = None;
                form.active()
                    .handle_event(&crossterm::event::Event::Key(*key));
            }
            Some(Overlay::Config(form)) => {
                let input = if form.editing_key {
                    &mut form.key_input
                } else {
                    &mut form.url_input
                };
                input.handle_event(&crossterm::event::Event::Key(*key));
            }
            _ => {}
        },
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
            match app.overlay.as_mut() {
                Some(Overlay::Help) => scroll_help(app, action),
                Some(Overlay::Passphrase(form)) => {
                    form.on_confirm = !form.on_confirm;
                }
                _ => edit_config_form(app, action),
            }
            None
        }
        Action::Confirm => confirm(app),
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
    if let Some(Overlay::Config(form)) = app.overlay.as_mut()
        && (form.editing_key || form.editing_url)
    {
        form.key_input.reset();
        form.url_input.reset();
        form.editing_key = false;
        form.editing_url = false;
        return None;
    }
    if app.overlay.is_some() {
        app.pending_keys.clear();
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
    open_tree_focused(app, app.tree.current);
}

fn open_tree_focused(app: &mut App, focus: Option<NodeId>) {
    if app.tree.nodes.is_empty() {
        app.notice = Some("no research yet".to_string());
        return;
    }
    app.tree_sel = focus
        .and_then(|id| app.tree.flatten().iter().position(|row| row.id == id))
        .unwrap_or(0);
    app.screen = Screen::Tree;
}

#[derive(Debug, PartialEq, Eq)]
enum SearchFailureNav {
    Home,
    Tree { focus: Option<NodeId> },
}

fn search_failure_nav(
    tree_empty: bool,
    pending_parent: Option<NodeId>,
    current: Option<NodeId>,
) -> SearchFailureNav {
    if tree_empty {
        SearchFailureNav::Home
    } else {
        SearchFailureNav::Tree {
            focus: pending_parent.or(current),
        }
    }
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
    specs: Vec<&'static EngineSpec>,
    model_counts: Vec<usize>,
    configured_model_idxs: Vec<usize>,
}

impl FormContext {
    fn from_app(app: &App) -> Self {
        Self {
            statuses_len: app.statuses.len(),
            specs: app.statuses.iter().map(|status| status.spec).collect(),
            model_counts: app
                .statuses
                .iter()
                .map(|status| model_choices(&app.config, status).len())
                .collect(),
            configured_model_idxs: app
                .statuses
                .iter()
                .map(|status| configured_model_idx(&app.config, status))
                .collect(),
        }
    }

    fn spec_at(&self, engine_idx: usize) -> Option<&'static EngineSpec> {
        self.specs.get(engine_idx).copied()
    }

    fn visible_len(&self, engine_idx: usize) -> usize {
        visible_config_fields(self.spec_at(engine_idx)).len()
    }
}

fn edit_config_form(app: &mut App, action: Action) {
    let context = FormContext::from_app(app);
    let Some(Overlay::Config(form)) = app.overlay.as_mut() else {
        return;
    };
    if form.editing_key || form.editing_url {
        return;
    }
    let len = context.visible_len(form.engine_idx);
    match action {
        Action::FieldNext => form.field_idx = (form.field_idx + 1) % len,
        Action::FieldPrev => form.field_idx = (form.field_idx + len - 1) % len,
        Action::ValueNext => step_config_value(form, 1, &context),
        Action::ValuePrev => step_config_value(form, -1, &context),
        _ => {}
    }
}

fn step_config_value(form: &mut ConfigForm, step: i8, context: &FormContext) {
    match form.field_of(context.spec_at(form.engine_idx)) {
        ConfigField::Language => {
            form.language_idx = cycle(form.language_idx, LANGUAGES.len(), step);
        }
        ConfigField::Engine => {
            form.engine_idx = cycle(form.engine_idx, context.statuses_len, step);
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
        ConfigField::ApiKey | ConfigField::BaseUrl => {}
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

fn confirm(app: &mut App) -> Option<Command> {
    match app.overlay {
        Some(Overlay::Passphrase(_)) => confirm_passphrase(app),
        Some(Overlay::Config(_)) => confirm_config(app),
        _ => None,
    }
}

fn confirm_config(app: &mut App) -> Option<Command> {
    if let Some(Overlay::Config(form)) = app.overlay.as_mut() {
        let spec = app.statuses.get(form.engine_idx).map(|status| status.spec);
        if form.editing_key || form.editing_url {
            form.editing_key = false;
            form.editing_url = false;
        } else {
            match form.field_of(spec) {
                ConfigField::ApiKey => {
                    form.editing_key = true;
                    return None;
                }
                ConfigField::BaseUrl => {
                    form.editing_url = true;
                    return None;
                }
                _ => {}
            }
        }
    }
    let Some(Overlay::Config(form)) = app.overlay.clone() else {
        return None;
    };
    let statuses = app.statuses.clone();
    form.apply_to(&mut app.config, &statuses);
    if let Some((engine, key)) = form.typed_key(&statuses) {
        app.pending_keys.insert(engine, key);
    }
    if app.pending_keys.is_empty() {
        app.overlay = None;
        return Some(Command::SaveConfig);
    }
    if let Some(passphrase) = app.passphrase.clone() {
        app.overlay = None;
        return Some(Command::UnlockVault { passphrase });
    }
    app.overlay = Some(Overlay::Passphrase(PassphraseForm::new(
        app.vaulted.is_empty(),
    )));
    None
}

fn confirm_passphrase(app: &mut App) -> Option<Command> {
    let Some(Overlay::Passphrase(form)) = app.overlay.as_mut() else {
        return None;
    };
    if form.creating && !form.on_confirm && form.confirm.value().is_empty() {
        form.on_confirm = true;
        return None;
    }
    match form.validated() {
        Ok(passphrase) => {
            app.overlay = None;
            Some(Command::UnlockVault {
                passphrase: Passphrase::new(passphrase),
            })
        }
        Err(message) => {
            form.error = Some(message.to_string());
            None
        }
    }
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
            match search_failure_nav(
                app.tree.nodes.is_empty(),
                app.pending_parent,
                app.tree.current,
            ) {
                SearchFailureNav::Home => {
                    app.pending_parent = None;
                    app.screen = Screen::Home;
                }
                SearchFailureNav::Tree { focus } => open_tree_focused(app, focus),
            }
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
                key_from_env: false,
                spec,
                available: index < 2,
                path: (index < 2).then(|| PathBuf::from("/fake/bin")),
                endpoint: None,
                models: spec.models.iter().map(ToString::to_string).collect(),
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
    fn search_failure_nav_chooses_home_or_tree() {
        struct Case {
            name: &'static str,
            tree_empty: bool,
            pending_parent: Option<NodeId>,
            current: Option<NodeId>,
            want: SearchFailureNav,
        }
        let cases = [
            Case {
                name: "empty tree goes home",
                tree_empty: true,
                pending_parent: Some(7),
                current: Some(3),
                want: SearchFailureNav::Home,
            },
            Case {
                name: "tree prefers pending parent as focus",
                tree_empty: false,
                pending_parent: Some(7),
                current: Some(3),
                want: SearchFailureNav::Tree { focus: Some(7) },
            },
            Case {
                name: "tree without pending focuses current",
                tree_empty: false,
                pending_parent: None,
                current: Some(3),
                want: SearchFailureNav::Tree { focus: Some(3) },
            },
            Case {
                name: "tree with neither focus stays unfocused",
                tree_empty: false,
                pending_parent: None,
                current: None,
                want: SearchFailureNav::Tree { focus: None },
            },
        ];
        for case in cases {
            assert_eq!(
                search_failure_nav(case.tree_empty, case.pending_parent, case.current),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn search_failure_applies_nav_and_notice() {
        struct Case {
            name: &'static str,
            with_tree: bool,
            set_pending: bool,
            want_screen: Screen,
            want_pending_kept: bool,
        }
        let cases = [
            Case {
                name: "empty tree returns home and clears pending",
                with_tree: false,
                set_pending: false,
                want_screen: Screen::Home,
                want_pending_kept: false,
            },
            Case {
                name: "existing tree opens tree and keeps pending",
                with_tree: true,
                set_pending: true,
                want_screen: Screen::Tree,
                want_pending_kept: true,
            },
        ];
        for case in cases {
            let mut app = if case.with_tree {
                searched_app()
            } else {
                app()
            };
            let pending = app.tree.current;
            if case.set_pending {
                app.pending_parent = pending;
            }
            app.begin_search();
            update(
                &mut app,
                AppEvent::Search(SearchEvent::Failed("boom".to_string())),
            );
            assert_eq!(app.screen, case.want_screen, "{}", case.name);
            assert_eq!(app.notice.as_deref(), Some("boom"), "{}", case.name);
            assert_eq!(
                app.pending_parent.is_some(),
                case.want_pending_kept,
                "{}",
                case.name
            );
            if case.want_pending_kept {
                assert_eq!(app.pending_parent, pending, "{}", case.name);
                assert_eq!(
                    app.tree.flatten().get(app.tree_sel).map(|row| row.id),
                    pending,
                    "{}",
                    case.name
                );
            }
        }
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

    fn typed(app: &mut App, text: &str) {
        for character in text.chars() {
            update(app, key(KeyCode::Char(character)));
        }
    }

    fn open_config_on(app: &mut App, engine: &str) {
        update(app, ctrl('o'));
        let index = app
            .statuses
            .iter()
            .position(|status| status.spec.name == engine)
            .expect("engine is in the table");
        let Some(Overlay::Config(form)) = app.overlay.as_mut() else {
            panic!("config overlay is open");
        };
        form.engine_idx = index;
    }

    #[test]
    fn entering_a_key_stashes_it_and_asks_for_a_passphrase() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        typed(&mut app, "sk-ant-secret");
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        assert_eq!(
            app.pending_keys.get("anthropic").map(String::as_str),
            Some("sk-ant-secret")
        );
        assert!(matches!(app.overlay, Some(Overlay::Passphrase(_))));
    }

    #[test]
    fn saving_the_config_never_writes_key_material_to_the_config_file() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-ant-donotleak");
        update(&mut app, key(KeyCode::Enter));
        let rendered = crate::core::config::to_toml(&app.config);
        assert!(!rendered.contains("sk-ant-donotleak"), "{rendered}");
        assert!(!rendered.contains("sk-"), "{rendered}");
    }

    #[test]
    fn typed_key_characters_never_reach_the_query_input() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-jjkk");
        assert_eq!(app.input.value(), "");
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        assert_eq!(form.key_input.value(), "sk-jjkk");
    }

    #[test]
    fn escape_while_editing_discards_the_key_without_closing_the_modal() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-ant-secret");
        assert_eq!(update(&mut app, key(KeyCode::Esc)), None);
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay stays open");
        };
        assert_eq!(form.key_input.value(), "");
        assert!(!form.editing_key);
        assert!(app.pending_keys.is_empty());
    }

    #[test]
    fn the_modal_only_shows_the_fields_the_selected_engine_can_use() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            want_key: bool,
            want_url: bool,
        }
        let cases = [
            Case {
                name: "a cli engine has neither",
                engine: "claude",
                want_key: false,
                want_url: false,
            },
            Case {
                name: "a keyless local server is addressable only",
                engine: "ollama",
                want_key: false,
                want_url: true,
            },
            Case {
                name: "a hosted api takes both",
                engine: "anthropic",
                want_key: true,
                want_url: true,
            },
        ];
        for case in cases {
            let mut app = app();
            open_config_on(&mut app, case.engine);
            let labels = visible_labels(&app);
            assert_eq!(
                labels.contains(&"api key"),
                case.want_key,
                "{}: {labels:?}",
                case.name
            );
            assert_eq!(
                labels.contains(&"base url"),
                case.want_url,
                "{}: {labels:?}",
                case.name
            );
            assert!(labels.contains(&"model"), "{}: {labels:?}", case.name);
            assert!(labels.contains(&"language"), "{}: {labels:?}", case.name);
        }
    }

    #[test]
    fn walking_the_fields_of_an_api_engine_wraps_only_after_its_extra_rows() {
        let mut app = app();
        open_config_on(&mut app, "openai");
        let count = visible_labels(&app).len();
        assert_eq!(
            count,
            8,
            "openai shows both api rows: {:?}",
            visible_labels(&app)
        );
        let mut visited = Vec::new();
        for _ in 0..count {
            let Some(Overlay::Config(form)) = &app.overlay else {
                panic!("config overlay is open");
            };
            visited.push(form.field(&app.statuses));
            update(&mut app, key(KeyCode::Down));
        }
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        assert_eq!(
            form.field_idx, 0,
            "the walk wraps after the last visible row"
        );
        assert!(visited.contains(&ConfigField::ApiKey), "{visited:?}");
        assert!(visited.contains(&ConfigField::BaseUrl), "{visited:?}");
    }

    #[test]
    fn walking_the_fields_backwards_wraps_onto_the_last_visible_row() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            rows: usize,
        }
        let cases = [
            Case {
                name: "a cli engine wraps onto its sixth row",
                engine: "claude",
                rows: 6,
            },
            Case {
                name: "an api engine wraps onto its eighth row",
                engine: "openai",
                rows: 8,
            },
        ];
        for case in cases {
            let mut app = app();
            open_config_on(&mut app, case.engine);
            assert_eq!(visible_labels(&app).len(), case.rows, "{}", case.name);
            update(&mut app, key(KeyCode::Up));
            let Some(Overlay::Config(form)) = &app.overlay else {
                panic!("config overlay is open");
            };
            assert_eq!(form.field_idx, case.rows - 1, "{}", case.name);
            assert_eq!(
                form.field(&app.statuses),
                ConfigField::MaxParallel,
                "{}",
                case.name
            );
            update(&mut app, key(KeyCode::Up));
            let Some(Overlay::Config(form)) = &app.overlay else {
                panic!("config overlay is open");
            };
            assert_eq!(
                form.field(&app.statuses),
                ConfigField::WebSearch,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn arrows_on_a_text_row_never_reach_the_toggle_below_it() {
        let mut app = app();
        open_config_on(&mut app, "openai");
        go_to_field(&mut app, ConfigField::ApiKey);
        let before = match &app.overlay {
            Some(Overlay::Config(form)) => form.clone(),
            _ => panic!("config overlay is open"),
        };
        update(&mut app, key(KeyCode::Right));
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        assert_eq!(*form, before, "an arrow on the key row must be inert");
    }

    #[test]
    fn a_base_url_typed_into_the_modal_is_saved_without_a_passphrase() {
        let mut app = app();
        open_config_on(&mut app, "local");
        go_to_field(&mut app, ConfigField::BaseUrl);
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        typed(&mut app, "http://127.0.0.1:1234");
        assert_eq!(
            update(&mut app, key(KeyCode::Enter)),
            Some(Command::SaveConfig)
        );
        assert_eq!(
            app.config.base_url_override("local"),
            Some("http://127.0.0.1:1234")
        );
        assert!(app.pending_keys.is_empty());
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn base_url_characters_never_reach_the_query_input() {
        let mut app = app();
        open_config_on(&mut app, "ollama");
        go_to_field(&mut app, ConfigField::BaseUrl);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "http://host:1");
        assert_eq!(app.input.value(), "");
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        assert_eq!(form.url_input.value(), "http://host:1");
        assert_eq!(form.key_input.value(), "");
    }

    #[test]
    fn escape_while_editing_the_base_url_discards_it_and_keeps_the_modal_open() {
        let mut app = app();
        open_config_on(&mut app, "local");
        go_to_field(&mut app, ConfigField::BaseUrl);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "http://127.0.0.1:1234");
        assert_eq!(update(&mut app, key(KeyCode::Esc)), None);
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay stays open");
        };
        assert_eq!(form.url_input.value(), "");
        assert!(!form.editing_url);
        assert_eq!(app.config.base_url_override("local"), None);
    }

    #[test]
    fn tab_while_editing_text_does_not_move_off_the_field() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            field: ConfigField,
        }
        let cases = [
            Case {
                name: "editing a key",
                engine: "anthropic",
                field: ConfigField::ApiKey,
            },
            Case {
                name: "editing a base url",
                engine: "local",
                field: ConfigField::BaseUrl,
            },
        ];
        for case in cases {
            let mut app = app();
            open_config_on(&mut app, case.engine);
            go_to_field(&mut app, case.field);
            update(&mut app, key(KeyCode::Enter));
            let before = match &app.overlay {
                Some(Overlay::Config(form)) => form.field_idx,
                _ => panic!("config overlay is open"),
            };
            update(&mut app, key(KeyCode::Tab));
            let Some(Overlay::Config(form)) = &app.overlay else {
                panic!("config overlay is open");
            };
            assert_eq!(form.field_idx, before, "{}", case.name);
            assert_eq!(form.field(&app.statuses), case.field, "{}", case.name);
        }
    }

    #[test]
    fn a_cli_engine_saves_straight_from_the_last_field() {
        let mut app = app();
        open_config_on(&mut app, "claude");
        go_to_field(&mut app, ConfigField::MaxParallel);
        assert_eq!(
            update(&mut app, key(KeyCode::Enter)),
            Some(Command::SaveConfig)
        );
        assert_eq!(app.overlay, None);
        assert!(app.pending_keys.is_empty());
    }

    #[test]
    fn walking_past_the_last_field_wraps_within_the_visible_ones() {
        let mut app = app();
        open_config_on(&mut app, "claude");
        let count = visible_labels(&app).len();
        for _ in 0..count {
            update(&mut app, key(KeyCode::Down));
        }
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        assert_eq!(form.field_idx, 0);
        assert_eq!(form.field(&app.statuses), ConfigField::Language);
    }

    #[test]
    fn saving_without_a_typed_key_never_asks_for_a_passphrase() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            update(&mut app, key(KeyCode::Enter)),
            Some(Command::SaveConfig)
        );
        assert!(app.pending_keys.is_empty());
    }

    #[test]
    fn a_cached_passphrase_skips_the_prompt_on_the_next_key() {
        let mut app = app();
        app.passphrase = Some(Passphrase::new("already unlocked"));
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-ant-secret");
        assert_eq!(
            update(&mut app, key(KeyCode::Enter)),
            Some(Command::UnlockVault {
                passphrase: Passphrase::new("already unlocked"),
            })
        );
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn tab_moves_between_the_passphrase_fields_and_back() {
        let mut app = app();
        app.overlay = Some(Overlay::Passphrase(PassphraseForm::new(true)));
        typed(&mut app, "one");
        update(&mut app, key(KeyCode::Tab));
        typed(&mut app, "two");
        update(&mut app, key(KeyCode::Tab));
        typed(&mut app, "three");
        let Some(Overlay::Passphrase(form)) = &app.overlay else {
            panic!("passphrase overlay is open");
        };
        assert_eq!(form.input.value(), "onethree");
        assert_eq!(form.confirm.value(), "two");
        assert!(!form.on_confirm);
    }

    #[test]
    fn the_passphrase_modal_confirms_a_new_vault_twice() {
        let mut app = app();
        app.overlay = Some(Overlay::Passphrase(PassphraseForm::new(true)));
        typed(&mut app, "hunter2");
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        typed(&mut app, "hunter3");
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        let Some(Overlay::Passphrase(form)) = &app.overlay else {
            panic!("passphrase overlay stays open");
        };
        assert_eq!(
            form.error.as_deref(),
            Some("the two passphrases do not match")
        );
    }

    #[test]
    fn a_matching_passphrase_unlocks_the_vault() {
        let mut app = app();
        app.overlay = Some(Overlay::Passphrase(PassphraseForm::new(true)));
        typed(&mut app, "hunter2");
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "hunter2");
        assert_eq!(
            update(&mut app, key(KeyCode::Enter)),
            Some(Command::UnlockVault {
                passphrase: Passphrase::new("hunter2"),
            })
        );
        assert_eq!(app.overlay, None);
    }

    #[test]
    fn an_empty_passphrase_is_refused() {
        let mut app = app();
        app.overlay = Some(Overlay::Passphrase(PassphraseForm::new(false)));
        assert_eq!(update(&mut app, key(KeyCode::Enter)), None);
        let Some(Overlay::Passphrase(form)) = &app.overlay else {
            panic!("passphrase overlay stays open");
        };
        assert_eq!(form.error.as_deref(), Some("passphrase must not be empty"));
    }

    #[test]
    fn cancelling_the_passphrase_drops_the_stashed_key() {
        let mut app = app();
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-ant-secret");
        update(&mut app, key(KeyCode::Enter));
        assert!(matches!(app.overlay, Some(Overlay::Passphrase(_))));
        assert_eq!(update(&mut app, key(KeyCode::Esc)), None);
        assert_eq!(app.overlay, None);
        assert!(app.pending_keys.is_empty());
    }

    #[test]
    fn neither_the_form_nor_the_command_prints_the_secret() {
        let mut app = app();
        app.passphrase = Some(Passphrase::new("open sesame"));
        open_config_on(&mut app, "anthropic");
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-ant-secret");
        let rendered = format!("{:?}", app.overlay);
        assert!(!rendered.contains("sk-ant-secret"), "{rendered}");
        let command = update(&mut app, key(KeyCode::Enter));
        let rendered = format!("{command:?}");
        assert!(!rendered.contains("open sesame"), "{rendered}");
    }

    fn visible_labels(app: &App) -> Vec<&'static str> {
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        form.visible_fields(&app.statuses)
            .iter()
            .map(|spec| spec.label)
            .collect()
    }

    fn go_to_field(app: &mut App, field: ConfigField) {
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay is open");
        };
        let fields = form.visible_fields(&app.statuses);
        let target = fields
            .iter()
            .position(|spec| spec.field == field)
            .expect("field is visible for the selected engine");
        let current = form.field_idx % fields.len();
        for _ in 0..(target + fields.len() - current) % fields.len() {
            update(app, key(KeyCode::Down));
        }
    }

    #[test]
    fn config_modal_edits_and_saves_settings() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        assert!(matches!(app.overlay, Some(Overlay::Config(_))));
        update(&mut app, key(KeyCode::Right));
        go_to_field(&mut app, ConfigField::ValidateLinks);
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
        go_to_field(&mut app, ConfigField::WebSearch);
        update(&mut app, key(KeyCode::Right));
        let command = update(&mut app, key(KeyCode::Enter));
        assert_eq!(command, Some(Command::SaveConfig));
        assert!(!app.config.websearch.enabled);
        update(&mut app, ctrl('o'));
        go_to_field(&mut app, ConfigField::WebSearch);
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

    fn selected_engine(app: &App) -> &'static str {
        let Some(Overlay::Config(form)) = &app.overlay else {
            panic!("config overlay open");
        };
        app.statuses[form.engine_idx].spec.name
    }

    #[test]
    fn engine_cycling_reaches_every_engine_including_unconfigured_ones() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        update(&mut app, key(KeyCode::Down));
        let mut seen = vec![selected_engine(&app)];
        for _ in 1..app.statuses.len() {
            update(&mut app, key(KeyCode::Right));
            seen.push(selected_engine(&app));
        }
        for engine in ENGINES {
            assert!(
                seen.contains(&engine.name),
                "{} must be reachable: {seen:?}",
                engine.name
            );
        }
        update(&mut app, key(KeyCode::Right));
        assert_eq!(selected_engine(&app), "claude", "the list wraps around");
    }

    #[test]
    fn cycling_backwards_from_the_first_engine_lands_on_the_last() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        update(&mut app, key(KeyCode::Down));
        update(&mut app, key(KeyCode::Left));
        let last = ENGINES.last().expect("the table is not empty").name;
        assert_eq!(selected_engine(&app), last);
    }

    #[test]
    fn an_unconfigured_api_engine_can_be_selected_and_given_a_key() {
        let mut app = app();
        update(&mut app, ctrl('o'));
        update(&mut app, key(KeyCode::Down));
        for _ in 0..app.statuses.len() {
            if selected_engine(&app) == "openai" {
                break;
            }
            update(&mut app, key(KeyCode::Right));
        }
        assert_eq!(selected_engine(&app), "openai");
        assert!(
            !app.statuses
                .iter()
                .any(|status| status.spec.name == "openai" && status.available),
            "the regression only bites while openai is unavailable"
        );
        go_to_field(&mut app, ConfigField::ApiKey);
        update(&mut app, key(KeyCode::Enter));
        typed(&mut app, "sk-openai-secret");
        update(&mut app, key(KeyCode::Enter));
        assert_eq!(
            app.pending_keys.get("openai").map(String::as_str),
            Some("sk-openai-secret")
        );
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
