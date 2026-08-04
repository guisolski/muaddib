use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MIN_PARALLEL: u8 = 1;
pub const MAX_PARALLEL: u8 = 8;
pub const MAX_BREADTH: u8 = 8;
pub const MODE_DEFAULT_BREADTH: u8 = 0;
pub const MIN_FAST_TIMEOUT_SECS: u64 = 5;
pub const MAX_FAST_TIMEOUT_SECS: u64 = 120;
pub const MIN_WEB_HITS: u8 = 1;
pub const MAX_WEB_HITS: u8 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub language: String,
    pub engine: String,
    pub max_parallel: u8,
    pub expansion_breadth: u8,
    pub validate_links: bool,
    pub images: bool,
    pub animations: bool,
    pub engine_timeout_secs: u64,
    pub fast_timeout_secs: u64,
    pub websearch: WebSearchConfig,
    pub engines: BTreeMap<String, EngineOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSearchConfig {
    pub enabled: bool,
    pub merge_snippets: bool,
    pub max_hits_per_query: u8,
    pub engines: Vec<String>,
    pub mailto: String,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            merge_snippets: false,
            max_hits_per_query: 5,
            engines: Vec::new(),
            mailto: String::new(),
        }
    }
}

impl WebSearchConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineOverride {
    pub bin: Option<PathBuf>,
    pub model: Option<String>,
    pub fast_model: Option<String>,
}

impl EngineOverride {
    fn is_empty(&self) -> bool {
        self.bin.is_none() && self.model.is_none() && self.fast_model.is_none()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "pt-BR".to_string(),
            engine: "claude".to_string(),
            max_parallel: 4,
            expansion_breadth: MODE_DEFAULT_BREADTH,
            validate_links: true,
            images: true,
            animations: true,
            engine_timeout_secs: 180,
            fast_timeout_secs: 45,
            websearch: WebSearchConfig::default(),
            engines: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn bin_override(&self, engine_name: &str) -> Option<&Path> {
        self.engines
            .get(engine_name)
            .and_then(|entry| entry.bin.as_deref())
    }

    pub fn model_override(&self, engine_name: &str) -> Option<&str> {
        self.engines
            .get(engine_name)
            .and_then(|entry| entry.model.as_deref())
    }

    pub fn fast_model_override(&self, engine_name: &str) -> Option<&str> {
        self.engines
            .get(engine_name)
            .and_then(|entry| entry.fast_model.as_deref())
    }

    pub fn set_model_override(&mut self, engine_name: &str, model: Option<String>) {
        let entry = self.engines.entry(engine_name.to_string()).or_default();
        entry.model = model;
        if entry.is_empty() {
            self.engines.remove(engine_name);
        }
    }
}

pub fn parse_config(toml_text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(toml_text).map(clamp_config)
}

pub fn clamp_config(mut config: Config) -> Config {
    config.max_parallel = config.max_parallel.clamp(MIN_PARALLEL, MAX_PARALLEL);
    config.expansion_breadth = config.expansion_breadth.min(MAX_BREADTH);
    config.fast_timeout_secs = config
        .fast_timeout_secs
        .clamp(MIN_FAST_TIMEOUT_SECS, MAX_FAST_TIMEOUT_SECS);
    config.websearch.max_hits_per_query = config
        .websearch
        .max_hits_per_query
        .clamp(MIN_WEB_HITS, MAX_WEB_HITS);
    config
}

pub fn to_toml(config: &Config) -> String {
    toml::to_string_pretty(config).unwrap_or_default()
}

pub fn config_path(home: &Path, xdg_config_home: Option<&Path>) -> PathBuf {
    xdg_config_home
        .map_or_else(|| home.join(".config"), Path::to_path_buf)
        .join("muaddib")
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_yields_defaults() {
        let config = parse_config("").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn partial_file_keeps_defaults_for_missing_keys() {
        let config = parse_config("language = \"en\"").unwrap();
        assert_eq!(config.language, "en");
        assert_eq!(config.engine, Config::default().engine);
        assert_eq!(config.max_parallel, Config::default().max_parallel);
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let config = parse_config("future_flag = true\nlanguage = \"es\"").unwrap();
        assert_eq!(config.language, "es");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want_parallel: u8,
            want_breadth: u8,
        }
        let cases = [
            Case {
                name: "parallel too high",
                input: "max_parallel = 99",
                want_parallel: MAX_PARALLEL,
                want_breadth: MODE_DEFAULT_BREADTH,
            },
            Case {
                name: "parallel zero",
                input: "max_parallel = 0",
                want_parallel: MIN_PARALLEL,
                want_breadth: MODE_DEFAULT_BREADTH,
            },
            Case {
                name: "breadth too high",
                input: "expansion_breadth = 50",
                want_parallel: 4,
                want_breadth: MAX_BREADTH,
            },
            Case {
                name: "breadth zero means mode default",
                input: "expansion_breadth = 0",
                want_parallel: 4,
                want_breadth: MODE_DEFAULT_BREADTH,
            },
        ];
        for case in cases {
            let config = parse_config(case.input).unwrap();
            assert_eq!(config.max_parallel, case.want_parallel, "{}", case.name);
            assert_eq!(config.expansion_breadth, case.want_breadth, "{}", case.name);
        }
    }

    #[test]
    fn images_flag_parses_and_defaults_true() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "absent flag defaults to true",
                input: "",
                want: true,
            },
            Case {
                name: "explicit false disables image fetching",
                input: "images = false",
                want: false,
            },
        ];
        for case in cases {
            let config = parse_config(case.input).unwrap();
            assert_eq!(config.images, case.want, "{}", case.name);
        }
    }

    #[test]
    fn animations_flag_parses_and_defaults_true() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "absent flag defaults to true",
                input: "",
                want: true,
            },
            Case {
                name: "explicit false disables animations",
                input: "animations = false",
                want: false,
            },
        ];
        for case in cases {
            let config = parse_config(case.input).unwrap();
            assert_eq!(config.animations, case.want, "{}", case.name);
        }
    }

    #[test]
    fn fast_timeout_parses_defaults_and_clamps() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: u64,
        }
        let cases = [
            Case {
                name: "absent key uses the default",
                input: "",
                want: 45,
            },
            Case {
                name: "explicit value is kept",
                input: "fast_timeout_secs = 30",
                want: 30,
            },
            Case {
                name: "too low is clamped up",
                input: "fast_timeout_secs = 0",
                want: MIN_FAST_TIMEOUT_SECS,
            },
            Case {
                name: "too high is clamped down",
                input: "fast_timeout_secs = 9000",
                want: MAX_FAST_TIMEOUT_SECS,
            },
        ];
        for case in cases {
            let config = parse_config(case.input).unwrap();
            assert_eq!(config.fast_timeout_secs, case.want, "{}", case.name);
        }
    }

    #[test]
    fn websearch_section_parses_defaults_and_clamps() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want_enabled: bool,
            want_merge: bool,
            want_hits: u8,
        }
        let cases = [
            Case {
                name: "absent section uses defaults",
                input: "",
                want_enabled: true,
                want_merge: false,
                want_hits: 5,
            },
            Case {
                name: "partial section keeps other defaults",
                input: "[websearch]\nenabled = false",
                want_enabled: false,
                want_merge: false,
                want_hits: 5,
            },
            Case {
                name: "merge snippets opt in",
                input: "[websearch]\nmerge_snippets = true",
                want_enabled: true,
                want_merge: true,
                want_hits: 5,
            },
            Case {
                name: "hits too high are clamped down",
                input: "[websearch]\nmax_hits_per_query = 99",
                want_enabled: true,
                want_merge: false,
                want_hits: MAX_WEB_HITS,
            },
            Case {
                name: "hits zero is clamped up",
                input: "[websearch]\nmax_hits_per_query = 0",
                want_enabled: true,
                want_merge: false,
                want_hits: MIN_WEB_HITS,
            },
        ];
        for case in cases {
            let config = parse_config(case.input).unwrap();
            assert_eq!(config.websearch.enabled, case.want_enabled, "{}", case.name);
            assert_eq!(
                config.websearch.merge_snippets, case.want_merge,
                "{}",
                case.name
            );
            assert_eq!(
                config.websearch.max_hits_per_query, case.want_hits,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn websearch_engines_and_mailto_parse_from_toml() {
        let text = "[websearch]\nengines = [\"ddg\", \"openalex\"]\nmailto = \"user@example.com\"";
        let config = parse_config(text).unwrap();
        assert_eq!(config.websearch.engines, vec!["ddg", "openalex"]);
        assert_eq!(config.websearch.mailto, "user@example.com");
    }

    #[test]
    fn websearch_disabled_helper_only_turns_the_feature_off() {
        let disabled = WebSearchConfig::disabled();
        assert!(!disabled.enabled);
        assert_eq!(
            WebSearchConfig {
                enabled: true,
                ..disabled
            },
            WebSearchConfig::default()
        );
    }

    #[test]
    fn engine_fast_model_override_round_trips() {
        let text = "[engines.claude]\nmodel = \"opus\"\nfast_model = \"haiku\"";
        let config = parse_config(text).unwrap();
        assert_eq!(config.fast_model_override("claude"), Some("haiku"));
        assert_eq!(config.model_override("claude"), Some("opus"));
        assert_eq!(config.fast_model_override("codex"), None);
    }

    #[test]
    fn clearing_the_model_keeps_a_fast_model_entry() {
        let mut config =
            parse_config("[engines.claude]\nmodel = \"opus\"\nfast_model = \"haiku\"").unwrap();
        config.set_model_override("claude", None);
        assert_eq!(config.model_override("claude"), None);
        assert_eq!(config.fast_model_override("claude"), Some("haiku"));
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(parse_config("language = ").is_err());
    }

    #[test]
    fn engine_bin_override_round_trips() {
        let text = "[engines.claude]\nbin = \"/custom/claude\"";
        let config = parse_config(text).unwrap();
        assert_eq!(
            config.bin_override("claude"),
            Some(Path::new("/custom/claude"))
        );
        assert_eq!(config.bin_override("codex"), None);
    }

    #[test]
    fn engine_model_override_round_trips() {
        let text = "[engines.claude]\nmodel = \"sonnet\"";
        let config = parse_config(text).unwrap();
        assert_eq!(config.model_override("claude"), Some("sonnet"));
        assert_eq!(config.model_override("codex"), None);
    }

    #[test]
    fn set_model_override_adds_updates_and_removes_entries() {
        struct Case {
            name: &'static str,
            initial: Option<&'static str>,
            set: Option<&'static str>,
            want: Option<&'static str>,
            want_entry: bool,
        }
        let cases = [
            Case {
                name: "set on empty config",
                initial: None,
                set: Some("opus"),
                want: Some("opus"),
                want_entry: true,
            },
            Case {
                name: "replace existing model",
                initial: Some("sonnet"),
                set: Some("haiku"),
                want: Some("haiku"),
                want_entry: true,
            },
            Case {
                name: "clearing removes the empty entry",
                initial: Some("sonnet"),
                set: None,
                want: None,
                want_entry: false,
            },
        ];
        for case in cases {
            let mut config = Config::default();
            if let Some(model) = case.initial {
                config.set_model_override("claude", Some(model.to_string()));
            }
            config.set_model_override("claude", case.set.map(String::from));
            assert_eq!(config.model_override("claude"), case.want, "{}", case.name);
            assert_eq!(
                config.engines.contains_key("claude"),
                case.want_entry,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn clearing_the_model_keeps_a_bin_override_entry() {
        let mut config = parse_config("[engines.claude]\nbin = \"/x\"\nmodel = \"opus\"").unwrap();
        config.set_model_override("claude", None);
        assert_eq!(config.model_override("claude"), None);
        assert_eq!(config.bin_override("claude"), Some(Path::new("/x")));
    }

    #[test]
    fn config_serializes_and_parses_back_identically() {
        let config = Config {
            language: "fr".to_string(),
            websearch: WebSearchConfig {
                enabled: false,
                merge_snippets: true,
                max_hits_per_query: 7,
                engines: vec!["ddg".to_string(), "openalex".to_string()],
                mailto: "user@example.com".to_string(),
            },
            engines: BTreeMap::from([(
                "claude".to_string(),
                EngineOverride {
                    bin: Some(PathBuf::from("/tmp/fake")),
                    model: Some("sonnet".to_string()),
                    fast_model: Some("haiku".to_string()),
                },
            )]),
            ..Config::default()
        };
        let round_tripped = parse_config(&to_toml(&config)).unwrap();
        assert_eq!(round_tripped, config);
    }

    #[test]
    fn config_path_prefers_xdg_config_home() {
        struct Case {
            name: &'static str,
            home: &'static str,
            xdg: Option<&'static str>,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "xdg set",
                home: "/home/user",
                xdg: Some("/home/user/.cfg"),
                want: "/home/user/.cfg/muaddib/config.toml",
            },
            Case {
                name: "xdg unset",
                home: "/home/user",
                xdg: None,
                want: "/home/user/.config/muaddib/config.toml",
            },
        ];
        for case in cases {
            let got = config_path(Path::new(case.home), case.xdg.map(Path::new));
            assert_eq!(got, PathBuf::from(case.want), "{}", case.name);
        }
    }
}
