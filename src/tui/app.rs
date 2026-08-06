use crate::core::config::{Config, MAX_PARALLEL, MIN_PARALLEL};
use crate::core::engine::EngineSpec;
use crate::core::mode::{MODES, Mode};
use crate::core::tree::{NodeId, ResearchTree};
use crate::core::vault::{Passphrase, mask};
use crate::engines::EngineStatus;
use crate::tui::images::ImageRuntime;
use crate::tui::search_state::SearchState;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
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
    Tree,
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
    FollowUp(FollowUpForm),
    Passphrase(PassphraseForm),
}

#[derive(Clone, Default)]
pub struct PassphraseForm {
    pub input: Input,
    pub confirm: Input,
    pub on_confirm: bool,
    pub creating: bool,
    pub error: Option<String>,
}

impl fmt::Debug for PassphraseForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PassphraseForm")
            .field("input", &"***")
            .field("confirm", &"***")
            .field("on_confirm", &self.on_confirm)
            .field("creating", &self.creating)
            .field("error", &self.error)
            .finish()
    }
}

impl PartialEq for PassphraseForm {
    fn eq(&self, other: &Self) -> bool {
        self.input.value() == other.input.value()
            && self.confirm.value() == other.confirm.value()
            && self.on_confirm == other.on_confirm
            && self.creating == other.creating
            && self.error == other.error
    }
}

impl Eq for PassphraseForm {}

impl PassphraseForm {
    pub fn new(creating: bool) -> Self {
        Self {
            creating,
            ..Self::default()
        }
    }

    pub fn active(&mut self) -> &mut Input {
        if self.creating && self.on_confirm {
            &mut self.confirm
        } else {
            &mut self.input
        }
    }

    pub fn validated(&self) -> Result<String, &'static str> {
        let value = self.input.value();
        if value.is_empty() {
            return Err("passphrase must not be empty");
        }
        if self.creating && value != self.confirm.value() {
            return Err("the two passphrases do not match");
        }
        Ok(value.to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct FollowUpForm {
    pub input: Input,
    pub parent: NodeId,
}

impl PartialEq for FollowUpForm {
    fn eq(&self, other: &Self) -> bool {
        self.parent == other.parent
            && self.input.value() == other.input.value()
            && self.input.cursor() == other.input.cursor()
    }
}

impl Eq for FollowUpForm {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Language,
    Engine,
    Model,
    ApiKey,
    BaseUrl,
    ValidateLinks,
    WebSearch,
    MaxParallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldVisibility {
    Always,
    Authenticated,
    Addressable,
}

impl FieldVisibility {
    fn applies_to(self, spec: Option<&EngineSpec>) -> bool {
        match self {
            Self::Always => true,
            Self::Authenticated => spec.is_some_and(takes_api_key),
            Self::Addressable => spec.is_some_and(takes_base_url),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ConfigFieldSpec {
    pub field: ConfigField,
    pub label: &'static str,
    pub visibility: FieldVisibility,
}

pub const CONFIG_FIELDS: &[ConfigFieldSpec] = &[
    ConfigFieldSpec {
        field: ConfigField::Language,
        label: "language",
        visibility: FieldVisibility::Always,
    },
    ConfigFieldSpec {
        field: ConfigField::Engine,
        label: "engine",
        visibility: FieldVisibility::Always,
    },
    ConfigFieldSpec {
        field: ConfigField::Model,
        label: "model",
        visibility: FieldVisibility::Always,
    },
    ConfigFieldSpec {
        field: ConfigField::ApiKey,
        label: "api key",
        visibility: FieldVisibility::Authenticated,
    },
    ConfigFieldSpec {
        field: ConfigField::BaseUrl,
        label: "base url",
        visibility: FieldVisibility::Addressable,
    },
    ConfigFieldSpec {
        field: ConfigField::ValidateLinks,
        label: "validate links",
        visibility: FieldVisibility::Always,
    },
    ConfigFieldSpec {
        field: ConfigField::WebSearch,
        label: "web search",
        visibility: FieldVisibility::Always,
    },
    ConfigFieldSpec {
        field: ConfigField::MaxParallel,
        label: "max parallel",
        visibility: FieldVisibility::Always,
    },
];

pub fn visible_config_fields(spec: Option<&EngineSpec>) -> Vec<ConfigFieldSpec> {
    CONFIG_FIELDS
        .iter()
        .filter(|field| field.visibility.applies_to(spec))
        .copied()
        .collect()
}

pub const ENGINE_DEFAULT_MODEL: &str = "default";

pub fn model_choices(config: &Config, status: &EngineStatus) -> Vec<String> {
    let mut choices = vec![ENGINE_DEFAULT_MODEL.to_string()];
    choices.extend(status.offered_models());
    if let Some(configured) = config.model_override(status.spec.name)
        && !choices.iter().any(|choice| choice == configured)
    {
        choices.push(configured.to_string());
    }
    choices
}

pub fn configured_model_idx(config: &Config, status: &EngineStatus) -> usize {
    config
        .model_override(status.spec.name)
        .map_or(0, |configured| {
            model_choices(config, status)
                .iter()
                .position(|choice| choice == configured)
                .unwrap_or(0)
        })
}

pub fn takes_api_key(spec: &EngineSpec) -> bool {
    spec.api().is_some_and(|api| api.auth_header.is_some())
}

pub fn takes_base_url(spec: &EngineSpec) -> bool {
    spec.api().is_some()
}

pub fn api_key_label(supported: bool, typed: &str, from_env: bool, vaulted: bool) -> String {
    if !supported {
        return "n/a".to_string();
    }
    if !typed.is_empty() {
        return mask(typed);
    }
    if from_env {
        return "from environment".to_string();
    }
    if vaulted {
        return "stored (encrypted)".to_string();
    }
    "not set".to_string()
}

pub fn base_url_label(supported: bool, typed: &str, endpoint: Option<&str>) -> String {
    if !supported {
        return "n/a".to_string();
    }
    if !typed.is_empty() {
        return typed.to_string();
    }
    endpoint.map_or_else(|| "not set".to_string(), ToString::to_string)
}

#[derive(Clone)]
pub struct ConfigForm {
    pub field_idx: usize,
    pub language_idx: usize,
    pub engine_idx: usize,
    pub model_idx: usize,
    pub key_input: Input,
    pub editing_key: bool,
    pub url_input: Input,
    pub editing_url: bool,
    pub validate_links: bool,
    pub websearch: bool,
    pub max_parallel: u8,
}

impl fmt::Debug for ConfigForm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigForm")
            .field("field_idx", &self.field_idx)
            .field("language_idx", &self.language_idx)
            .field("engine_idx", &self.engine_idx)
            .field("model_idx", &self.model_idx)
            .field("key_input", &"***")
            .field("editing_key", &self.editing_key)
            .field("url_input", &self.url_input.value())
            .field("editing_url", &self.editing_url)
            .field("validate_links", &self.validate_links)
            .field("websearch", &self.websearch)
            .field("max_parallel", &self.max_parallel)
            .finish()
    }
}

impl PartialEq for ConfigForm {
    fn eq(&self, other: &Self) -> bool {
        self.field_idx == other.field_idx
            && self.language_idx == other.language_idx
            && self.engine_idx == other.engine_idx
            && self.model_idx == other.model_idx
            && self.key_input.value() == other.key_input.value()
            && self.editing_key == other.editing_key
            && self.url_input.value() == other.url_input.value()
            && self.editing_url == other.editing_url
            && self.validate_links == other.validate_links
            && self.websearch == other.websearch
            && self.max_parallel == other.max_parallel
    }
}

impl Eq for ConfigForm {}

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
                .map_or(0, |status| configured_model_idx(config, status)),
            key_input: Input::default(),
            editing_key: false,
            url_input: Input::default(),
            editing_url: false,
            validate_links: config.validate_links,
            websearch: config.websearch.enabled,
            max_parallel: config.max_parallel,
        }
    }

    pub fn typed_key(&self, statuses: &[EngineStatus]) -> Option<(String, String)> {
        let status = statuses.get(self.engine_idx)?;
        let value = self.key_input.value().trim();
        (takes_api_key(status.spec) && !value.is_empty())
            .then(|| (status.spec.name.to_string(), value.to_string()))
    }

    pub fn typed_base_url(&self, statuses: &[EngineStatus]) -> Option<(String, String)> {
        let status = statuses.get(self.engine_idx)?;
        let value = self.url_input.value().trim();
        (takes_base_url(status.spec) && !value.is_empty())
            .then(|| (status.spec.name.to_string(), value.to_string()))
    }

    pub fn visible_fields(&self, statuses: &[EngineStatus]) -> Vec<ConfigFieldSpec> {
        visible_config_fields(statuses.get(self.engine_idx).map(|status| status.spec))
    }

    pub fn field_of(&self, spec: Option<&EngineSpec>) -> ConfigField {
        let fields = visible_config_fields(spec);
        fields[self.field_idx % fields.len()].field
    }

    pub fn field(&self, statuses: &[EngineStatus]) -> ConfigField {
        self.field_of(statuses.get(self.engine_idx).map(|status| status.spec))
    }

    pub fn apply_to(&self, config: &mut Config, statuses: &[EngineStatus]) {
        let selected_model = statuses
            .get(self.engine_idx)
            .map(|status| (status.spec.name, self.chosen_model(config, status)));
        config.language = LANGUAGES[self.language_idx % LANGUAGES.len()].to_string();
        if let Some(status) = statuses.get(self.engine_idx) {
            config.engine = status.spec.name.to_string();
        }
        if let Some((engine_name, model)) = selected_model {
            config.set_model_override(engine_name, model);
        }
        if let Some((engine_name, base_url)) = self.typed_base_url(statuses) {
            config.set_base_url_override(&engine_name, Some(base_url));
        }
        config.validate_links = self.validate_links;
        config.websearch.enabled = self.websearch;
        config.max_parallel = self.max_parallel.clamp(MIN_PARALLEL, MAX_PARALLEL);
    }

    fn chosen_model(&self, config: &Config, status: &EngineStatus) -> Option<String> {
        let choices = model_choices(config, status);
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
    pub tree: ResearchTree,
    pub tree_sel: usize,
    pub help_scroll: u16,
    pub pending_parent: Option<NodeId>,
    pub session_path: Option<PathBuf>,
    pub clock_unix: u64,
    pub search_started_unix: u64,
    pub keys: BTreeMap<String, String>,
    pub pending_keys: BTreeMap<String, String>,
    pub vaulted: Vec<String>,
    pub passphrase: Option<Passphrase>,
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
            tree: ResearchTree::default(),
            tree_sel: 0,
            help_scroll: 0,
            pending_parent: None,
            session_path: None,
            clock_unix: 0,
            search_started_unix: 0,
            keys: BTreeMap::new(),
            pending_keys: BTreeMap::new(),
            vaulted: Vec::new(),
            passphrase: None,
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
        self.search_started_unix = self.clock_unix;
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
    use crate::engines::EngineSpec;

    fn claude_status() -> EngineStatus {
        EngineStatus::unavailable(claude_spec())
    }

    fn claude_spec() -> &'static EngineSpec {
        &ENGINES[0]
    }

    fn status_named(name: &str) -> EngineStatus {
        EngineStatus::unavailable(
            ENGINES
                .iter()
                .find(|spec| spec.name == name)
                .expect("engine is in the table"),
        )
    }

    #[test]
    fn only_engines_that_authenticate_offer_a_key_field() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "a hosted api takes a key",
                engine: "anthropic",
                want: true,
            },
            Case {
                name: "openai takes a key",
                engine: "openai",
                want: true,
            },
            Case {
                name: "ollama is keyless",
                engine: "ollama",
                want: false,
            },
            Case {
                name: "a cli engine takes no key",
                engine: "claude",
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(
                takes_api_key(status_named(case.engine).spec),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn only_engines_reached_over_http_offer_a_base_url_field() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "a keyless local server is addressable",
                engine: "ollama",
                want: true,
            },
            Case {
                name: "the generic openai-compatible engine is addressable",
                engine: "local",
                want: true,
            },
            Case {
                name: "a hosted api can be pointed at a proxy",
                engine: "anthropic",
                want: true,
            },
            Case {
                name: "a cli engine has no endpoint",
                engine: "claude",
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(
                takes_base_url(status_named(case.engine).spec),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn the_key_field_reports_where_the_key_comes_from() {
        struct Case {
            name: &'static str,
            supported: bool,
            typed: &'static str,
            from_env: bool,
            vaulted: bool,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "an engine without keys",
                supported: false,
                typed: "",
                from_env: false,
                vaulted: false,
                want: "n/a",
            },
            Case {
                name: "nothing configured",
                supported: true,
                typed: "",
                from_env: false,
                vaulted: false,
                want: "not set",
            },
            Case {
                name: "an environment variable",
                supported: true,
                typed: "",
                from_env: true,
                vaulted: false,
                want: "from environment",
            },
            Case {
                name: "the vault",
                supported: true,
                typed: "",
                from_env: false,
                vaulted: true,
                want: "stored (encrypted)",
            },
            Case {
                name: "the environment wins over the vault",
                supported: true,
                typed: "",
                from_env: true,
                vaulted: true,
                want: "from environment",
            },
            Case {
                name: "a key being typed is masked",
                supported: true,
                typed: "sk-ant-api03-secret-4f2a",
                from_env: true,
                vaulted: true,
                want: "••••••••4f2a",
            },
        ];
        for case in cases {
            assert_eq!(
                api_key_label(case.supported, case.typed, case.from_env, case.vaulted),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn a_typed_key_is_claimed_only_by_an_engine_that_authenticates() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            typed: &'static str,
            want: Option<(&'static str, &'static str)>,
        }
        let cases = [
            Case {
                name: "a hosted api claims the key",
                engine: "anthropic",
                typed: "  sk-ant-1  ",
                want: Some(("anthropic", "sk-ant-1")),
            },
            Case {
                name: "blank input is not a key",
                engine: "anthropic",
                typed: "   ",
                want: None,
            },
            Case {
                name: "a cli engine never claims a key",
                engine: "claude",
                typed: "sk-ant-1",
                want: None,
            },
        ];
        for case in cases {
            let statuses = vec![status_named(case.engine)];
            let mut form = ConfigForm::from_state(&Config::default(), &statuses);
            form.key_input = Input::new(case.typed.to_string());
            assert_eq!(
                form.typed_key(&statuses),
                case.want
                    .map(|(engine, key)| (engine.to_string(), key.to_string())),
                "{}",
                case.name
            );
        }
    }

    fn labels_for(engine: Option<&'static str>) -> Vec<&'static str> {
        let status = engine.map(status_named);
        visible_config_fields(status.as_ref().map(|status| status.spec))
            .iter()
            .map(|spec| spec.label)
            .collect()
    }

    #[test]
    fn the_visible_fields_follow_what_the_selected_engine_can_use() {
        struct Case {
            name: &'static str,
            engine: Option<&'static str>,
            want: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "a cli engine hides both api rows",
                engine: Some("claude"),
                want: &[
                    "language",
                    "engine",
                    "model",
                    "validate links",
                    "web search",
                    "max parallel",
                ],
            },
            Case {
                name: "a keyless local server shows only the base url",
                engine: Some("ollama"),
                want: &[
                    "language",
                    "engine",
                    "model",
                    "base url",
                    "validate links",
                    "web search",
                    "max parallel",
                ],
            },
            Case {
                name: "a hosted api shows both, key before url",
                engine: Some("openai"),
                want: &[
                    "language",
                    "engine",
                    "model",
                    "api key",
                    "base url",
                    "validate links",
                    "web search",
                    "max parallel",
                ],
            },
            Case {
                name: "no engine at all falls back to the shared rows",
                engine: None,
                want: &[
                    "language",
                    "engine",
                    "model",
                    "validate links",
                    "web search",
                    "max parallel",
                ],
            },
        ];
        for case in cases {
            assert_eq!(labels_for(case.engine), case.want, "{}", case.name);
        }
    }

    #[test]
    fn every_engine_keeps_the_rows_that_are_always_visible() {
        for spec in ENGINES {
            let labels = labels_for(Some(spec.name));
            for always in ["language", "engine", "model", "max parallel"] {
                assert!(labels.contains(&always), "{}: {labels:?}", spec.name);
            }
            assert_eq!(labels[0], "language", "{}", spec.name);
            assert_eq!(labels[1], "engine", "{}", spec.name);
        }
    }

    #[test]
    fn the_selected_field_shifts_with_the_rows_the_engine_shows() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            field_idx: usize,
            want: ConfigField,
        }
        let cases = [
            Case {
                name: "row three is the api key on a hosted api",
                engine: "openai",
                field_idx: 3,
                want: ConfigField::ApiKey,
            },
            Case {
                name: "row three is the base url on a keyless server",
                engine: "ollama",
                field_idx: 3,
                want: ConfigField::BaseUrl,
            },
            Case {
                name: "row three is a toggle on a cli engine",
                engine: "claude",
                field_idx: 3,
                want: ConfigField::ValidateLinks,
            },
            Case {
                name: "the index wraps within the visible rows",
                engine: "claude",
                field_idx: 6,
                want: ConfigField::Language,
            },
        ];
        for case in cases {
            let status = status_named(case.engine);
            let mut form =
                ConfigForm::from_state(&Config::default(), std::slice::from_ref(&status));
            form.field_idx = case.field_idx;
            assert_eq!(form.field_of(Some(status.spec)), case.want, "{}", case.name);
        }
    }

    #[test]
    fn the_base_url_field_reports_the_endpoint_in_use() {
        struct Case {
            name: &'static str,
            supported: bool,
            typed: &'static str,
            endpoint: Option<&'static str>,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "a cli engine has no endpoint",
                supported: false,
                typed: "",
                endpoint: None,
                want: "n/a",
            },
            Case {
                name: "an engine still waiting for an address",
                supported: true,
                typed: "",
                endpoint: None,
                want: "not set",
            },
            Case {
                name: "the resolved endpoint shows through",
                supported: true,
                typed: "",
                endpoint: Some("http://localhost:11434"),
                want: "http://localhost:11434",
            },
            Case {
                name: "what was typed wins over the resolved endpoint",
                supported: true,
                typed: "http://127.0.0.1:1234",
                endpoint: Some("http://localhost:11434"),
                want: "http://127.0.0.1:1234",
            },
            Case {
                name: "typing into an unsupported engine changes nothing",
                supported: false,
                typed: "http://127.0.0.1:1234",
                endpoint: Some("http://localhost:11434"),
                want: "n/a",
            },
        ];
        for case in cases {
            assert_eq!(
                base_url_label(case.supported, case.typed, case.endpoint),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn a_typed_base_url_is_kept_only_for_engines_reached_over_http() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            typed: &'static str,
            want: Option<(&'static str, &'static str)>,
        }
        let cases = [
            Case {
                name: "a local server keeps it",
                engine: "local",
                typed: "http://127.0.0.1:1234",
                want: Some(("local", "http://127.0.0.1:1234")),
            },
            Case {
                name: "surrounding blanks are trimmed",
                engine: "ollama",
                typed: "  http://127.0.0.1:11434  ",
                want: Some(("ollama", "http://127.0.0.1:11434")),
            },
            Case {
                name: "a blank entry is not an override",
                engine: "local",
                typed: "   ",
                want: None,
            },
            Case {
                name: "a cli engine never takes one",
                engine: "claude",
                typed: "http://127.0.0.1:1234",
                want: None,
            },
        ];
        for case in cases {
            let statuses = vec![status_named(case.engine)];
            let mut form = ConfigForm::from_state(&Config::default(), &statuses);
            form.url_input = Input::new(case.typed.to_string());
            assert_eq!(
                form.typed_base_url(&statuses),
                case.want
                    .map(|(engine, url)| (engine.to_string(), url.to_string())),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn applying_the_form_writes_the_typed_base_url_into_the_config() {
        let statuses = vec![status_named("local")];
        let mut form = ConfigForm::from_state(&Config::default(), &statuses);
        form.url_input = Input::new("http://127.0.0.1:1234".to_string());
        let mut config = Config::default();
        form.apply_to(&mut config, &statuses);
        assert_eq!(config.engine, "local");
        assert_eq!(
            config.base_url_override("local"),
            Some("http://127.0.0.1:1234")
        );
    }

    #[test]
    fn applying_the_form_leaves_an_untouched_base_url_alone() {
        let statuses = vec![status_named("local")];
        let form = ConfigForm::from_state(&Config::default(), &statuses);
        let mut config = Config::default();
        config.set_base_url_override("local", Some("http://already:1234".to_string()));
        form.apply_to(&mut config, &statuses);
        assert_eq!(
            config.base_url_override("local"),
            Some("http://already:1234")
        );
    }

    #[test]
    fn the_config_form_never_prints_the_typed_key() {
        let statuses = vec![status_named("anthropic")];
        let mut form = ConfigForm::from_state(&Config::default(), &statuses);
        form.key_input = Input::new("sk-ant-donotleak".to_string());
        form.url_input = Input::new("http://127.0.0.1:1234".to_string());
        let rendered = format!("{form:?}");
        assert!(!rendered.contains("sk-ant-donotleak"), "{rendered}");
        assert!(rendered.starts_with("ConfigForm {"), "{rendered}");
        assert!(rendered.contains("key_input: \"***\""), "{rendered}");
        assert!(
            rendered.contains("url_input: \"http://127.0.0.1:1234\""),
            "a base url is not a secret: {rendered}"
        );
        assert!(rendered.contains("engine_idx: 0"), "{rendered}");
    }

    #[test]
    fn the_passphrase_form_never_prints_either_field() {
        let mut form = PassphraseForm::new(true);
        form.input = Input::new("open sesame".to_string());
        form.confirm = Input::new("open sesame".to_string());
        let rendered = format!("{form:?}");
        assert!(!rendered.contains("open sesame"), "{rendered}");
        assert!(rendered.starts_with("PassphraseForm {"), "{rendered}");
        assert!(rendered.contains("input: \"***\""), "{rendered}");
        assert!(rendered.contains("confirm: \"***\""), "{rendered}");
        assert!(rendered.contains("creating: true"), "{rendered}");
    }

    fn config_form() -> ConfigForm {
        ConfigForm {
            field_idx: 1,
            language_idx: 2,
            engine_idx: 3,
            model_idx: 4,
            key_input: Input::new("sk-1".to_string()),
            editing_key: false,
            url_input: Input::new("http://127.0.0.1:1234".to_string()),
            editing_url: false,
            validate_links: true,
            websearch: true,
            max_parallel: 4,
        }
    }

    #[test]
    fn two_config_forms_are_equal_only_when_every_field_matches() {
        struct Case {
            name: &'static str,
            mutate: fn(&mut ConfigForm),
        }
        let cases = [
            Case {
                name: "field_idx",
                mutate: |form| form.field_idx = 5,
            },
            Case {
                name: "language_idx",
                mutate: |form| form.language_idx = 5,
            },
            Case {
                name: "engine_idx",
                mutate: |form| form.engine_idx = 5,
            },
            Case {
                name: "model_idx",
                mutate: |form| form.model_idx = 5,
            },
            Case {
                name: "key_input",
                mutate: |form| form.key_input = Input::new("sk-2".to_string()),
            },
            Case {
                name: "editing_key",
                mutate: |form| form.editing_key = true,
            },
            Case {
                name: "url_input",
                mutate: |form| form.url_input = Input::new("http://127.0.0.1:9".to_string()),
            },
            Case {
                name: "editing_url",
                mutate: |form| form.editing_url = true,
            },
            Case {
                name: "validate_links",
                mutate: |form| form.validate_links = false,
            },
            Case {
                name: "websearch",
                mutate: |form| form.websearch = false,
            },
            Case {
                name: "max_parallel",
                mutate: |form| form.max_parallel = 8,
            },
        ];
        assert_eq!(config_form(), config_form());
        for case in cases {
            let mut other = config_form();
            (case.mutate)(&mut other);
            assert_ne!(config_form(), other, "differs only in {}", case.name);
        }
    }

    fn passphrase_form() -> PassphraseForm {
        PassphraseForm {
            input: Input::new("one".to_string()),
            confirm: Input::new("two".to_string()),
            on_confirm: false,
            creating: true,
            error: None,
        }
    }

    #[test]
    fn two_passphrase_forms_are_equal_only_when_every_field_matches() {
        struct Case {
            name: &'static str,
            mutate: fn(&mut PassphraseForm),
        }
        let cases = [
            Case {
                name: "input",
                mutate: |form| form.input = Input::new("other".to_string()),
            },
            Case {
                name: "confirm",
                mutate: |form| form.confirm = Input::new("other".to_string()),
            },
            Case {
                name: "on_confirm",
                mutate: |form| form.on_confirm = true,
            },
            Case {
                name: "creating",
                mutate: |form| form.creating = false,
            },
            Case {
                name: "error",
                mutate: |form| form.error = Some("boom".to_string()),
            },
        ];
        assert_eq!(passphrase_form(), passphrase_form());
        for case in cases {
            let mut other = passphrase_form();
            (case.mutate)(&mut other);
            assert_ne!(passphrase_form(), other, "differs only in {}", case.name);
        }
    }

    #[test]
    fn the_passphrase_form_validates_before_it_unlocks() {
        struct Case {
            name: &'static str,
            creating: bool,
            input: &'static str,
            confirm: &'static str,
            want: Result<&'static str, &'static str>,
        }
        let cases = [
            Case {
                name: "unlocking accepts any non-empty value",
                creating: false,
                input: "hunter2",
                confirm: "",
                want: Ok("hunter2"),
            },
            Case {
                name: "unlocking refuses an empty value",
                creating: false,
                input: "",
                confirm: "",
                want: Err("passphrase must not be empty"),
            },
            Case {
                name: "creating requires the two to match",
                creating: true,
                input: "hunter2",
                confirm: "hunter2",
                want: Ok("hunter2"),
            },
            Case {
                name: "creating refuses a mismatch",
                creating: true,
                input: "hunter2",
                confirm: "hunter3",
                want: Err("the two passphrases do not match"),
            },
            Case {
                name: "an empty passphrase is refused before the mismatch check",
                creating: true,
                input: "",
                confirm: "",
                want: Err("passphrase must not be empty"),
            },
        ];
        for case in cases {
            let mut form = PassphraseForm::new(case.creating);
            form.input = Input::new(case.input.to_string());
            form.confirm = Input::new(case.confirm.to_string());
            assert_eq!(
                form.validated().as_deref().map_err(|error| *error),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn the_active_passphrase_field_follows_the_selection() {
        let mut form = PassphraseForm::new(true);
        form.active()
            .handle(tui_input::InputRequest::InsertChar('a'));
        assert_eq!(form.input.value(), "a");
        assert_eq!(form.confirm.value(), "");
        form.on_confirm = true;
        form.active()
            .handle(tui_input::InputRequest::InsertChar('b'));
        assert_eq!(form.input.value(), "a");
        assert_eq!(form.confirm.value(), "b");
        let mut unlocking = PassphraseForm::new(false);
        unlocking.on_confirm = true;
        unlocking
            .active()
            .handle(tui_input::InputRequest::InsertChar('c'));
        assert_eq!(
            unlocking.input.value(),
            "c",
            "unlocking has no confirm field to move to"
        );
    }

    #[test]
    fn model_choices_start_with_default_then_curated_models() {
        let choices = model_choices(&Config::default(), &claude_status());
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
        let choices = model_choices(&config, &claude_status());
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
                configured_model_idx(&config, &claude_status()),
                case.want,
                "{}",
                case.name
            );
        }
    }
}
