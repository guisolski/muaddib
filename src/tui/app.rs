use crate::core::config::{Config, MAX_PARALLEL, MIN_PARALLEL};
use crate::core::mode::{MODES, Mode};
use crate::engines::{EngineSpec, EngineStatus};
use crate::tui::images::ImageRuntime;
use crate::tui::search_state::SearchState;
use std::time::Instant;
use tui_input::Input;

pub const LANGUAGES: &[&str] = &["en", "pt-BR", "es", "fr", "de", "it", "ja", "zh"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 80,
            height: 24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Searching,
    Results,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Body,
    Sources(usize),
    Followups(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Config(ConfigForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Language,
    Engine,
    Model,
    ValidateLinks,
    WebSearch,
    MaxParallel,
}

#[derive(Debug, Clone, Copy)]
pub struct ConfigFieldSpec {
    pub field: ConfigField,
    pub label: &'static str,
}

pub const CONFIG_FIELDS: &[ConfigFieldSpec] = &[
    ConfigFieldSpec {
        field: ConfigField::Language,
        label: "language",
    },
    ConfigFieldSpec {
        field: ConfigField::Engine,
        label: "engine",
    },
    ConfigFieldSpec {
        field: ConfigField::Model,
        label: "model",
    },
    ConfigFieldSpec {
        field: ConfigField::ValidateLinks,
        label: "validate links",
    },
    ConfigFieldSpec {
        field: ConfigField::WebSearch,
        label: "web search",
    },
    ConfigFieldSpec {
        field: ConfigField::MaxParallel,
        label: "max parallel",
    },
];

pub const ENGINE_DEFAULT_MODEL: &str = "default";

pub fn model_choices(config: &Config, spec: &'static EngineSpec) -> Vec<String> {
    let mut choices = vec![ENGINE_DEFAULT_MODEL.to_string()];
    choices.extend(spec.models.iter().map(ToString::to_string));
    if let Some(configured) = config.model_override(spec.name)
        && !choices.iter().any(|choice| choice == configured)
    {
        choices.push(configured.to_string());
    }
    choices
}

pub fn configured_model_idx(config: &Config, spec: &'static EngineSpec) -> usize {
    config.model_override(spec.name).map_or(0, |configured| {
        model_choices(config, spec)
            .iter()
            .position(|choice| choice == configured)
            .unwrap_or(0)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigForm {
    pub field_idx: usize,
    pub language_idx: usize,
    pub engine_idx: usize,
    pub model_idx: usize,
    pub validate_links: bool,
    pub websearch: bool,
    pub max_parallel: u8,
}

impl ConfigForm {
    pub fn from_state(config: &Config, statuses: &[EngineStatus]) -> Self {
        let engine_idx = statuses
            .iter()
            .position(|status| status.spec.name == config.engine)
            .unwrap_or(0);
        Self {
            field_idx: 0,
            language_idx: LANGUAGES
                .iter()
                .position(|lang| *lang == config.language)
                .unwrap_or(0),
            engine_idx,
            model_idx: statuses
                .get(engine_idx)
                .map_or(0, |status| configured_model_idx(config, status.spec)),
            validate_links: config.validate_links,
            websearch: config.websearch.enabled,
            max_parallel: config.max_parallel,
        }
    }

    pub fn field(&self) -> ConfigField {
        CONFIG_FIELDS[self.field_idx % CONFIG_FIELDS.len()].field
    }

    pub fn apply_to(&self, config: &mut Config, statuses: &[EngineStatus]) {
        let selected_model = statuses
            .get(self.engine_idx)
            .map(|status| (status.spec.name, self.chosen_model(config, status.spec)));
        config.language = LANGUAGES[self.language_idx % LANGUAGES.len()].to_string();
        if let Some(status) = statuses.get(self.engine_idx) {
            config.engine = status.spec.name.to_string();
        }
        if let Some((engine_name, model)) = selected_model {
            config.set_model_override(engine_name, model);
        }
        config.validate_links = self.validate_links;
        config.websearch.enabled = self.websearch;
        config.max_parallel = self.max_parallel.clamp(MIN_PARALLEL, MAX_PARALLEL);
    }

    fn chosen_model(&self, config: &Config, spec: &'static EngineSpec) -> Option<String> {
        let choices = model_choices(config, spec);
        let choice = choices.get(self.model_idx % choices.len())?;
        (choice != ENGINE_DEFAULT_MODEL).then(|| choice.clone())
    }
}

pub struct App {
    pub config: Config,
    pub statuses: Vec<EngineStatus>,
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub input: Input,
    pub mode_idx: usize,
    pub fast: bool,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub history_draft: String,
    pub clear_history_armed: bool,
    pub search: SearchState,
    pub image_runtime: ImageRuntime,
    pub viewport: Viewport,
    pub scroll: u16,
    pub focus: Focus,
    pub tick: u64,
    pub notice: Option<String>,
}

impl App {
    pub fn new(
        config: Config,
        statuses: Vec<EngineStatus>,
        initial_mode: Option<Mode>,
        fast: bool,
    ) -> Self {
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
            fast,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
            clear_history_armed: false,
            search: SearchState::default(),
            image_runtime: ImageRuntime::default(),
            viewport: Viewport::default(),
            scroll: 0,
            focus: Focus::Body,
            tick: 0,
            notice: None,
        }
    }

    pub fn current_mode(&self) -> Mode {
        MODES[self.mode_idx % MODES.len()].mode
    }

    pub fn begin_search(&mut self) {
        self.screen = Screen::Searching;
        self.search.begin(Instant::now());
        self.image_runtime.clear();
        self.scroll = 0;
        self.focus = Focus::Body;
        self.notice = None;
    }

    pub fn end_search(&mut self) {
        self.search.end();
    }

    pub fn selected_engine(&self) -> Option<&EngineStatus> {
        self.statuses
            .iter()
            .find(|status| status.spec.name == self.config.engine)
    }

    pub fn source_count(&self) -> usize {
        self.search
            .answer
            .as_ref()
            .map_or(0, |answer| answer.sources.len())
    }

    pub fn followup_count(&self) -> usize {
        self.search
            .answer
            .as_ref()
            .map_or(0, |answer| answer.followups.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::ENGINES;

    fn claude_spec() -> &'static EngineSpec {
        &ENGINES[0]
    }

    #[test]
    fn model_choices_start_with_default_then_curated_models() {
        let choices = model_choices(&Config::default(), claude_spec());
        assert_eq!(choices[0], ENGINE_DEFAULT_MODEL);
        assert_eq!(choices.len(), claude_spec().models.len() + 1);
        for model in claude_spec().models {
            assert!(choices.iter().any(|choice| choice == model), "{model}");
        }
    }

    #[test]
    fn model_choices_include_a_custom_configured_model() {
        let mut config = Config::default();
        config.set_model_override("claude", Some("my-custom-model".to_string()));
        let choices = model_choices(&config, claude_spec());
        assert_eq!(choices.last().map(String::as_str), Some("my-custom-model"));
    }

    #[test]
    fn configured_model_idx_points_at_the_configured_model() {
        struct Case {
            name: &'static str,
            configured: Option<&'static str>,
            want: usize,
        }
        let cases = [
            Case {
                name: "no override selects default",
                configured: None,
                want: 0,
            },
            Case {
                name: "curated override selects its row",
                configured: Some("sonnet"),
                want: 2,
            },
            Case {
                name: "custom override selects the appended row",
                configured: Some("my-custom-model"),
                want: 4,
            },
        ];
        for case in cases {
            let mut config = Config::default();
            config.set_model_override("claude", case.configured.map(String::from));
            assert_eq!(
                configured_model_idx(&config, claude_spec()),
                case.want,
                "{}",
                case.name
            );
        }
    }
}
