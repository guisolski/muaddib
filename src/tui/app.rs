use crate::core::answer::Answer;
use crate::core::config::{Config, MAX_PARALLEL, MIN_PARALLEL};
use crate::core::mode::{MODES, Mode};
use crate::core::plan::SearchPlan;
use crate::engines::EngineStatus;
use crate::pipeline::{LinkStatus, SearchHandle};
use std::collections::HashMap;
use std::time::Instant;
use tui_input::Input;

pub const LANGUAGES: &[&str] = &["en", "pt-BR", "es", "fr", "de", "it", "ja", "zh"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Searching,
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Config(ConfigForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubQueryState {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Language,
    Engine,
    ValidateLinks,
    MaxParallel,
}

pub const CONFIG_FIELDS: &[ConfigField] = &[
    ConfigField::Language,
    ConfigField::Engine,
    ConfigField::ValidateLinks,
    ConfigField::MaxParallel,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigForm {
    pub field_idx: usize,
    pub language_idx: usize,
    pub engine_idx: usize,
    pub validate_links: bool,
    pub max_parallel: u8,
}

impl ConfigForm {
    pub fn from_state(config: &Config, statuses: &[EngineStatus]) -> Self {
        Self {
            field_idx: 0,
            language_idx: LANGUAGES
                .iter()
                .position(|lang| *lang == config.language)
                .unwrap_or(0),
            engine_idx: statuses
                .iter()
                .position(|status| status.spec.name == config.engine)
                .unwrap_or(0),
            validate_links: config.validate_links,
            max_parallel: config.max_parallel,
        }
    }

    pub fn field(&self) -> ConfigField {
        CONFIG_FIELDS[self.field_idx % CONFIG_FIELDS.len()]
    }

    pub fn apply_to(&self, config: &mut Config, statuses: &[EngineStatus]) {
        config.language = LANGUAGES[self.language_idx % LANGUAGES.len()].to_string();
        if let Some(status) = statuses.get(self.engine_idx) {
            config.engine = status.spec.name.to_string();
        }
        config.validate_links = self.validate_links;
        config.max_parallel = self.max_parallel.clamp(MIN_PARALLEL, MAX_PARALLEL);
    }
}

pub struct App {
    pub config: Config,
    pub statuses: Vec<EngineStatus>,
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub input: Input,
    pub mode_idx: usize,
    pub plan: Option<SearchPlan>,
    pub progress: Vec<SubQueryState>,
    pub synthesizing: bool,
    pub answer: Option<Answer>,
    pub links: HashMap<u32, LinkStatus>,
    pub scroll: u16,
    pub sources_focused: bool,
    pub selected_source: usize,
    pub tick: u64,
    pub notice: Option<String>,
    pub search: Option<SearchHandle>,
    pub started_at: Option<Instant>,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: Config, statuses: Vec<EngineStatus>, initial_mode: Option<Mode>) -> Self {
        let mode_idx = initial_mode
            .and_then(|mode| MODES.iter().position(|spec| spec.mode == mode))
            .unwrap_or(0);
        Self {
            config,
            statuses,
            screen: Screen::Home,
            overlay: None,
            input: Input::default(),
            mode_idx,
            plan: None,
            progress: Vec::new(),
            synthesizing: false,
            answer: None,
            links: HashMap::new(),
            scroll: 0,
            sources_focused: false,
            selected_source: 0,
            tick: 0,
            notice: None,
            search: None,
            started_at: None,
            should_quit: false,
        }
    }

    pub fn current_mode(&self) -> Mode {
        MODES[self.mode_idx % MODES.len()].mode
    }

    pub fn begin_search(&mut self) {
        self.screen = Screen::Searching;
        self.plan = None;
        self.progress.clear();
        self.synthesizing = false;
        self.answer = None;
        self.links.clear();
        self.scroll = 0;
        self.sources_focused = false;
        self.selected_source = 0;
        self.notice = None;
        self.started_at = Some(Instant::now());
    }

    pub fn end_search(&mut self) {
        self.search = None;
        self.synthesizing = false;
    }

    pub fn selected_engine(&self) -> Option<&EngineStatus> {
        self.statuses
            .iter()
            .find(|status| status.spec.name == self.config.engine)
    }

    pub fn source_count(&self) -> usize {
        self.answer
            .as_ref()
            .map_or(0, |answer| answer.sources.len())
    }
}
