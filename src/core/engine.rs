use crate::core::api::{ApiSpec, SchemaMode, Wire};
use crate::core::config::Config;
use crate::core::cost::{EngineUsage, ModelPrice, parse_usage};
use crate::core::stream::result_line;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineId {
    Claude,
    CursorAgent,
    Codex,
    Opencode,
    Ollama,
    Local,
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseStrategy {
    ClaudeJson,
    GenericJson,
    RawText,
}

#[derive(Debug)]
pub struct CliSpec {
    pub bin: &'static str,
    pub args: &'static [&'static str],
    pub streams: bool,
    pub parse: ParseStrategy,
    pub model_flag: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub enum Transport {
    Cli(&'static CliSpec),
    Api(&'static ApiSpec),
}

#[derive(Debug)]
pub struct EngineSpec {
    pub id: EngineId,
    pub prices: &'static [ModelPrice],
    pub name: &'static str,
    pub transport: Transport,
    pub supports_json_schema: bool,
    pub models: &'static [&'static str],
    pub fast_model: Option<&'static str>,
    pub auto_select: bool,
    pub missing_label: &'static str,
    pub install_hint: &'static str,
}

impl EngineSpec {
    pub fn cli(&self) -> Option<&'static CliSpec> {
        match self.transport {
            Transport::Cli(cli) => Some(cli),
            Transport::Api(_) => None,
        }
    }

    pub fn api(&self) -> Option<&'static ApiSpec> {
        match self.transport {
            Transport::Api(api) => Some(api),
            Transport::Cli(_) => None,
        }
    }
}

pub const STREAM_FORMAT_ARG: &str = "stream-json";

const CLAUDE_CLI: CliSpec = CliSpec {
    bin: "claude",
    args: &[
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--allowedTools=WebSearch,WebFetch",
    ],
    streams: true,
    parse: ParseStrategy::ClaudeJson,
    model_flag: Some("--model"),
};

const CURSOR_AGENT_CLI: CliSpec = CliSpec {
    bin: "cursor-agent",
    args: &["-p", "--output-format", "json"],
    streams: false,
    parse: ParseStrategy::GenericJson,
    model_flag: Some("--model"),
};

const CODEX_CLI: CliSpec = CliSpec {
    bin: "codex",
    args: &["exec", "--skip-git-repo-check"],
    streams: false,
    parse: ParseStrategy::RawText,
    model_flag: Some("--model"),
};

const OPENCODE_CLI: CliSpec = CliSpec {
    bin: "opencode",
    args: &["run"],
    streams: false,
    parse: ParseStrategy::RawText,
    model_flag: Some("--model"),
};

const OLLAMA_API: ApiSpec = ApiSpec {
    wire: Wire::OllamaChat,
    base_url: "http://localhost:11434",
    base_url_env: &["OLLAMA_HOST"],
    path: "/api/chat",
    models_path: "/api/tags",
    auth_header: None,
    auth_prefix: "",
    key_env: &[],
    extra_headers: &[],
    schema_mode: SchemaMode::JsonObject,
    default_max_tokens: 0,
    probes_models: true,
};

const LOCAL_API: ApiSpec = ApiSpec {
    wire: Wire::OpenAiChat,
    base_url: "",
    base_url_env: &["MUADDIB_LOCAL_BASE_URL", "OPENAI_BASE_URL"],
    path: "/v1/chat/completions",
    models_path: "/v1/models",
    auth_header: Some("authorization"),
    auth_prefix: "Bearer ",
    key_env: &["MUADDIB_LOCAL_API_KEY"],
    extra_headers: &[],
    schema_mode: SchemaMode::JsonObject,
    default_max_tokens: 0,
    probes_models: true,
};

const OPENAI_API: ApiSpec = ApiSpec {
    wire: Wire::OpenAiChat,
    base_url: "https://api.openai.com",
    base_url_env: &["OPENAI_BASE_URL"],
    path: "/v1/chat/completions",
    models_path: "/v1/models",
    auth_header: Some("authorization"),
    auth_prefix: "Bearer ",
    key_env: &["OPENAI_API_KEY"],
    extra_headers: &[],
    schema_mode: SchemaMode::JsonObject,
    default_max_tokens: 0,
    probes_models: false,
};

const ANTHROPIC_API: ApiSpec = ApiSpec {
    wire: Wire::AnthropicMessages,
    base_url: "https://api.anthropic.com",
    base_url_env: &["ANTHROPIC_BASE_URL"],
    path: "/v1/messages",
    models_path: "/v1/models",
    auth_header: Some("x-api-key"),
    auth_prefix: "",
    key_env: &["ANTHROPIC_API_KEY"],
    extra_headers: &[("anthropic-version", "2023-06-01")],
    schema_mode: SchemaMode::JsonObject,
    default_max_tokens: 16_384,
    probes_models: false,
};

const GEMINI_API: ApiSpec = ApiSpec {
    wire: Wire::GeminiGenerate,
    base_url: "https://generativelanguage.googleapis.com",
    base_url_env: &[],
    path: "/v1beta/models/{model}:generateContent",
    models_path: "/v1beta/models",
    auth_header: Some("x-goog-api-key"),
    auth_prefix: "",
    key_env: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
    extra_headers: &[],
    schema_mode: SchemaMode::JsonObject,
    default_max_tokens: 0,
    probes_models: false,
};

const OPENAI_PRICES: &[ModelPrice] = &[
    ModelPrice {
        prefix: "gpt-5-mini",
        input_per_million: 0.25,
        output_per_million: 2.00,
    },
    ModelPrice {
        prefix: "gpt-5",
        input_per_million: 1.25,
        output_per_million: 10.00,
    },
];

const ANTHROPIC_PRICES: &[ModelPrice] = &[
    ModelPrice {
        prefix: "claude-opus-5",
        input_per_million: 5.00,
        output_per_million: 25.00,
    },
    ModelPrice {
        prefix: "claude-sonnet-5",
        input_per_million: 3.00,
        output_per_million: 15.00,
    },
    ModelPrice {
        prefix: "claude-haiku-4-5",
        input_per_million: 1.00,
        output_per_million: 5.00,
    },
];

const GEMINI_PRICES: &[ModelPrice] = &[
    ModelPrice {
        prefix: "gemini-2.5-flash",
        input_per_million: 0.30,
        output_per_million: 2.50,
    },
    ModelPrice {
        prefix: "gemini-2.5-pro",
        input_per_million: 1.25,
        output_per_million: 10.00,
    },
];

pub const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: EngineId::Claude,
        prices: &[],
        name: "claude",
        transport: Transport::Cli(&CLAUDE_CLI),
        supports_json_schema: true,
        models: &["opus", "sonnet", "haiku"],
        fast_model: Some("haiku"),
        auto_select: true,
        missing_label: "not installed",
        install_hint: "npm install -g @anthropic-ai/claude-code",
    },
    EngineSpec {
        id: EngineId::CursorAgent,
        prices: &[],
        name: "cursor-agent",
        transport: Transport::Cli(&CURSOR_AGENT_CLI),
        supports_json_schema: false,
        models: &["auto", "gpt-5", "sonnet-4.5"],
        fast_model: None,
        auto_select: true,
        missing_label: "not installed",
        install_hint: "curl https://cursor.com/install -fsS | bash",
    },
    EngineSpec {
        id: EngineId::Codex,
        prices: &[],
        name: "codex",
        transport: Transport::Cli(&CODEX_CLI),
        supports_json_schema: false,
        models: &["gpt-5-codex", "gpt-5"],
        fast_model: None,
        auto_select: true,
        missing_label: "not installed",
        install_hint: "npm install -g @openai/codex",
    },
    EngineSpec {
        id: EngineId::Opencode,
        prices: &[],
        name: "opencode",
        transport: Transport::Cli(&OPENCODE_CLI),
        supports_json_schema: false,
        models: &["anthropic/claude-sonnet-4-5", "openai/gpt-5"],
        fast_model: None,
        auto_select: true,
        missing_label: "not installed",
        install_hint: "npm install -g opencode-ai",
    },
    EngineSpec {
        id: EngineId::Ollama,
        prices: &[],
        name: "ollama",
        transport: Transport::Api(&OLLAMA_API),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "not running",
        install_hint: "https://ollama.com/download, then: ollama pull qwen3:8b",
    },
    EngineSpec {
        id: EngineId::Local,
        prices: &[],
        name: "local",
        transport: Transport::Api(&LOCAL_API),
        supports_json_schema: false,
        models: &[],
        fast_model: None,
        auto_select: true,
        missing_label: "no base url",
        install_hint: "set [engines.local] base_url to any OpenAI-compatible server",
    },
    EngineSpec {
        id: EngineId::OpenAi,
        prices: OPENAI_PRICES,
        name: "openai",
        transport: Transport::Api(&OPENAI_API),
        supports_json_schema: false,
        models: &["gpt-5", "gpt-5-mini"],
        fast_model: Some("gpt-5-mini"),
        auto_select: false,
        missing_label: "no API key",
        install_hint: "export OPENAI_API_KEY=...",
    },
    EngineSpec {
        id: EngineId::Anthropic,
        prices: ANTHROPIC_PRICES,
        name: "anthropic",
        transport: Transport::Api(&ANTHROPIC_API),
        supports_json_schema: false,
        models: &["claude-sonnet-5", "claude-opus-5", "claude-haiku-4-5"],
        fast_model: Some("claude-haiku-4-5"),
        auto_select: false,
        missing_label: "no API key",
        install_hint: "export ANTHROPIC_API_KEY=...",
    },
    EngineSpec {
        id: EngineId::Gemini,
        prices: GEMINI_PRICES,
        name: "gemini",
        transport: Transport::Api(&GEMINI_API),
        supports_json_schema: false,
        models: &["gemini-2.5-flash", "gemini-2.5-pro"],
        fast_model: Some("gemini-2.5-flash"),
        auto_select: false,
        missing_label: "no API key",
        install_hint: "export GEMINI_API_KEY=...",
    },
];

pub fn engine_by_name(name: &str) -> Option<&'static EngineSpec> {
    ENGINES.iter().find(|spec| spec.name == name)
}

#[derive(Debug, Clone)]
pub struct EngineJob {
    pub prompt: String,
    pub schema: Option<&'static str>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EngineOutput {
    pub text: String,
    pub usage: Option<EngineUsage>,
}

impl EngineOutput {
    pub fn from_text(text: String) -> Self {
        Self { text, usage: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("engine timed out after {0:?}")]
    TimedOut(Duration),
    #[error("engine failed with status {status}: {stderr_tail}")]
    Failed { status: i32, stderr_tail: String },
    #[error("engine reported an error: {0}")]
    Reported(String),
    #[error("failed to spawn engine: {0}")]
    Spawn(String),
}

pub fn build_args(
    spec: &EngineSpec,
    cli: &CliSpec,
    model: Option<&str>,
    job: &EngineJob,
) -> Vec<String> {
    let mut args: Vec<String> = cli.args.iter().map(ToString::to_string).collect();
    if let (Some(flag), Some(model)) = (cli.model_flag, model) {
        args.push(flag.to_string());
        args.push(model.to_string());
    }
    if spec.supports_json_schema
        && let Some(schema) = job.schema
    {
        args.push("--json-schema".to_string());
        args.push(schema.to_string());
    }
    args.push(job.prompt.clone());
    args
}

#[derive(Debug, Clone)]
pub struct EngineStatus {
    pub spec: &'static EngineSpec,
    pub available: bool,
    pub path: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub models: Vec<String>,
    pub key_from_env: bool,
}

impl EngineStatus {
    pub fn unavailable(spec: &'static EngineSpec) -> Self {
        Self {
            spec,
            available: false,
            path: None,
            endpoint: None,
            models: spec.models.iter().map(ToString::to_string).collect(),
            key_from_env: false,
        }
    }

    pub fn offered_models(&self) -> Vec<String> {
        if self.models.is_empty() {
            self.spec.models.iter().map(ToString::to_string).collect()
        } else {
            self.models.clone()
        }
    }
}

pub fn resolve_model(config: &Config, status: &EngineStatus, fast: bool) -> Option<String> {
    if fast
        && let Some(model) = config
            .fast_model_override(status.spec.name)
            .or(status.spec.fast_model)
    {
        return Some(model.to_string());
    }
    config
        .model_override(status.spec.name)
        .map(str::to_string)
        .or_else(|| status.offered_models().first().cloned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no engine is available")]
pub struct NoEngineAvailable;

pub fn choose_engine<'a>(
    statuses: &'a [EngineStatus],
    requested: &str,
) -> Result<(&'a EngineStatus, Option<String>), NoEngineAvailable> {
    let exact = statuses
        .iter()
        .find(|status| status.spec.name == requested && status.available);
    if let Some(status) = exact {
        return Ok((status, None));
    }
    let fallback = statuses
        .iter()
        .find(|status| status.available && status.spec.auto_select)
        .or_else(|| statuses.iter().find(|status| status.available))
        .ok_or(NoEngineAvailable)?;
    let notice = format!(
        "engine '{requested}' is not available; using '{}'",
        fallback.spec.name
    );
    Ok((fallback, Some(notice)))
}

pub fn envelope_text(strategy: ParseStrategy, stdout: &str) -> Result<String, EngineError> {
    match strategy {
        ParseStrategy::ClaudeJson => claude_envelope_text(stdout),
        ParseStrategy::GenericJson => Ok(generic_envelope_text(stdout)),
        ParseStrategy::RawText => Ok(stdout.to_string()),
    }
}

pub fn envelope_output(strategy: ParseStrategy, stdout: &str) -> Result<EngineOutput, EngineError> {
    let text = envelope_text(strategy, stdout)?;
    Ok(EngineOutput {
        text,
        usage: envelope_usage(strategy, stdout),
    })
}

fn envelope_usage(strategy: ParseStrategy, stdout: &str) -> Option<EngineUsage> {
    if strategy != ParseStrategy::ClaudeJson {
        return None;
    }
    claude_envelope(stdout).as_ref().and_then(parse_usage)
}

fn claude_envelope(stdout: &str) -> Option<Value> {
    serde_json::from_str::<Value>(stdout.trim())
        .ok()
        .or_else(|| result_line(stdout).and_then(|line| serde_json::from_str::<Value>(line).ok()))
}

fn claude_envelope_text(stdout: &str) -> Result<String, EngineError> {
    let Some(envelope) = claude_envelope(stdout) else {
        return Ok(stdout.to_string());
    };
    if envelope.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err(EngineError::Reported(reported_error_text(&envelope)));
    }
    if let Some(structured) = envelope.get("structured_output")
        && !structured.is_null()
    {
        return Ok(structured.to_string());
    }
    Ok(probe_text_keys(&envelope).unwrap_or_else(|| stdout.to_string()))
}

fn reported_error_text(envelope: &Value) -> String {
    probe_text_keys(envelope).unwrap_or_else(|| "unknown engine error".to_string())
}

const GENERIC_TEXT_KEYS: &[&str] = &["result", "text", "response", "content", "message", "output"];

fn generic_envelope_text(stdout: &str) -> String {
    envelope_from_json_text(stdout)
        .or_else(|| envelope_from_last_line(stdout))
        .unwrap_or_else(|| stdout.to_string())
}

fn envelope_from_json_text(text: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(text.trim()).ok()?;
    probe_text_keys(&value)
}

fn envelope_from_last_line(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(envelope_from_json_text)
}

fn probe_text_keys(value: &Value) -> Option<String> {
    GENERIC_TEXT_KEYS.iter().find_map(|key| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::api::MODEL_PLACEHOLDER;
    use std::collections::BTreeSet;

    fn job(prompt: &str, schema: Option<&'static str>) -> EngineJob {
        EngineJob {
            prompt: prompt.to_string(),
            schema,
            timeout: Duration::from_secs(1),
        }
    }

    fn status(spec: &'static EngineSpec, available: bool) -> EngineStatus {
        EngineStatus {
            key_from_env: false,
            spec,
            available,
            path: available.then(|| PathBuf::from("/fake/bin")),
            endpoint: None,
            models: spec.models.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn engines_table_names_are_unique_and_resolvable() {
        let names: BTreeSet<&str> = ENGINES.iter().map(|spec| spec.name).collect();
        assert_eq!(names.len(), ENGINES.len());
        for spec in ENGINES {
            assert_eq!(engine_by_name(spec.name).unwrap().id, spec.id);
            assert!(!spec.install_hint.is_empty(), "{}", spec.name);
        }
    }

    #[test]
    fn every_engine_declares_exactly_one_transport() {
        for spec in ENGINES {
            assert_ne!(spec.cli().is_some(), spec.api().is_some(), "{}", spec.name);
        }
    }

    #[test]
    fn only_engines_asking_for_a_line_stream_claim_to_stream() {
        for spec in ENGINES {
            let Some(cli) = spec.cli() else { continue };
            assert_eq!(
                cli.streams,
                cli.args.contains(&STREAM_FORMAT_ARG),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn api_engines_declare_a_key_env_exactly_when_they_authenticate() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert_eq!(
                api.auth_header.is_none(),
                api.key_env.is_empty(),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn api_engines_interpolate_the_model_only_into_the_path() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert!(!api.base_url.contains(MODEL_PLACEHOLDER), "{}", spec.name);
            assert!(api.path.starts_with('/'), "{}", spec.name);
            assert!(api.models_path.starts_with('/'), "{}", spec.name);
        }
    }

    #[test]
    fn wires_that_require_a_token_budget_declare_a_default() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            let required = api.wire == Wire::AnthropicMessages;
            assert_eq!(api.default_max_tokens > 0, required, "{}", spec.name);
        }
    }

    #[test]
    fn no_api_engine_sends_the_draft_seven_schema_natively_yet() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert_ne!(api.schema_mode, SchemaMode::NativeSchema, "{}", spec.name);
        }
    }

    #[test]
    fn only_engines_that_probe_can_ship_without_a_model_list() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert!(
                api.probes_models || !spec.models.is_empty(),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn paid_api_engines_are_never_chosen_automatically() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert_eq!(
                spec.auto_select,
                api.auth_header.is_none() || api.probes_models,
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn only_configurable_api_engines_ship_without_a_base_url() {
        for spec in ENGINES {
            let Some(api) = spec.api() else { continue };
            assert!(
                api.base_url.is_empty() || api.base_url.starts_with("http"),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn a_streaming_claude_envelope_is_read_from_its_result_line() {
        let stdout = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,",
            "\"result\":\"the payload\",\"total_cost_usd\":0.25,",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n"
        );
        let output = envelope_output(ParseStrategy::ClaudeJson, stdout).unwrap();
        assert_eq!(output.text, "the payload");
        assert_eq!(
            output.usage,
            Some(EngineUsage {
                input_tokens: 10,
                output_tokens: 5,
                cost_usd: 0.25,
            })
        );
    }

    #[test]
    fn a_streaming_claude_error_still_surfaces_as_a_reported_error() {
        let stdout = concat!(
            "{\"type\":\"system\",\"subtype\":\"init\"}\n",
            "{\"type\":\"result\",\"subtype\":\"error_during_execution\",",
            "\"is_error\":true,\"result\":\"Credit balance is too low\"}\n"
        );
        let error = envelope_text(ParseStrategy::ClaudeJson, stdout).unwrap_err();
        assert_eq!(
            error,
            EngineError::Reported("Credit balance is too low".to_string())
        );
    }

    #[test]
    fn a_streaming_claude_answer_still_prefers_structured_output() {
        let stdout = concat!(
            "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,",
            "\"result\":\"{\\\"title\\\":\\\"raw\\\"}\",",
            "\"structured_output\":{\"title\":\"Structured\"}}\n"
        );
        let text = envelope_text(ParseStrategy::ClaudeJson, stdout).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["title"], "Structured");
    }

    #[test]
    fn a_stream_that_never_reached_its_result_passes_through_raw() {
        let stdout = "{\"type\":\"system\"}\n{\"type\":\"assistant\"}";
        let text = envelope_text(ParseStrategy::ClaudeJson, stdout).unwrap();
        assert_eq!(text, stdout);
    }

    #[test]
    fn build_args_places_the_prompt_last() {
        for spec in ENGINES {
            let Some(cli) = spec.cli() else { continue };
            let args = build_args(spec, cli, Some("some-model"), &job("the prompt", None));
            assert_eq!(
                args.last().map(String::as_str),
                Some("the prompt"),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn build_args_adds_the_model_flag_only_when_a_model_is_set() {
        for spec in ENGINES {
            let Some(cli) = spec.cli() else { continue };
            let with_model = build_args(spec, cli, Some("some-model"), &job("p", None));
            let without_model = build_args(spec, cli, None, &job("p", None));
            assert!(
                with_model.windows(2).any(|pair| {
                    pair[0] == cli.model_flag.unwrap_or_default() && pair[1] == "some-model"
                }),
                "{}",
                spec.name
            );
            assert!(
                !without_model.iter().any(|arg| arg == "some-model"),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn build_args_adds_schema_flag_only_for_supporting_engines() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            schema: Option<&'static str>,
            want_flag: bool,
        }
        let cases = [
            Case {
                name: "claude with schema",
                engine: "claude",
                schema: Some("{}"),
                want_flag: true,
            },
            Case {
                name: "claude without schema",
                engine: "claude",
                schema: None,
                want_flag: false,
            },
            Case {
                name: "codex with schema",
                engine: "codex",
                schema: Some("{}"),
                want_flag: false,
            },
        ];
        for case in cases {
            let spec = engine_by_name(case.engine).unwrap();
            let cli = spec.cli().unwrap();
            let args = build_args(spec, cli, None, &job("p", case.schema));
            assert_eq!(
                args.iter().any(|arg| arg == "--json-schema"),
                case.want_flag,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn resolve_model_follows_fast_then_config_then_the_offered_list() {
        struct Case {
            name: &'static str,
            toml: &'static str,
            fast: bool,
            discovered: &'static [&'static str],
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "no config falls back to the first curated model",
                toml: "",
                fast: false,
                discovered: &[],
                want: Some("opus"),
            },
            Case {
                name: "a configured model wins",
                toml: "[engines.claude]\nmodel = \"sonnet\"",
                fast: false,
                discovered: &[],
                want: Some("sonnet"),
            },
            Case {
                name: "fast mode prefers the table's fast model",
                toml: "[engines.claude]\nmodel = \"sonnet\"",
                fast: true,
                discovered: &[],
                want: Some("haiku"),
            },
            Case {
                name: "a configured fast model beats the table's",
                toml: "[engines.claude]\nfast_model = \"my-haiku\"",
                fast: true,
                discovered: &[],
                want: Some("my-haiku"),
            },
            Case {
                name: "outside fast mode the fast model is ignored",
                toml: "[engines.claude]\nfast_model = \"my-haiku\"",
                fast: false,
                discovered: &[],
                want: Some("opus"),
            },
            Case {
                name: "discovered models take the place of the curated list",
                toml: "",
                fast: false,
                discovered: &["qwen3:8b", "llama3.2"],
                want: Some("qwen3:8b"),
            },
        ];
        for case in cases {
            let config = crate::core::config::parse_config(case.toml).expect("config parses");
            let mut engine_status = status(&ENGINES[0], true);
            if !case.discovered.is_empty() {
                engine_status.models = case.discovered.iter().map(ToString::to_string).collect();
            }
            assert_eq!(
                resolve_model(&config, &engine_status, case.fast).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn an_engine_with_no_models_at_all_resolves_to_none() {
        let mut engine_status = status(&ENGINES[0], true);
        engine_status.models = Vec::new();
        let spec_without_models: &'static EngineSpec = ENGINES
            .iter()
            .find(|spec| spec.models.is_empty())
            .expect("a keyless probing engine ships no curated models");
        engine_status.spec = spec_without_models;
        assert_eq!(
            resolve_model(&Config::default(), &engine_status, false),
            None
        );
    }

    #[test]
    fn every_curated_model_of_a_paid_engine_has_a_price() {
        for spec in ENGINES {
            let paid = spec
                .api()
                .is_some_and(|api| api.auth_header.is_some() && !api.probes_models);
            assert_eq!(
                !spec.prices.is_empty(),
                paid,
                "{} carries prices iff it bills",
                spec.name
            );
            for model in spec.models {
                assert_eq!(
                    crate::core::cost::price_for(spec.prices, model).is_some(),
                    paid,
                    "{}: model {model}",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn choose_engine_prefers_the_requested_available_engine() {
        let statuses = vec![status(&ENGINES[0], true), status(&ENGINES[1], true)];
        let (chosen, notice) = choose_engine(&statuses, "cursor-agent").unwrap();
        assert_eq!(chosen.spec.name, "cursor-agent");
        assert!(notice.is_none());
    }

    #[test]
    fn choose_engine_falls_back_to_first_available_with_notice() {
        let statuses = vec![status(&ENGINES[0], false), status(&ENGINES[1], true)];
        let (chosen, notice) = choose_engine(&statuses, "claude").unwrap();
        assert_eq!(chosen.spec.name, "cursor-agent");
        assert!(notice.unwrap().contains("claude"));
    }

    #[test]
    fn choose_engine_errors_when_nothing_is_installed() {
        let statuses = vec![status(&ENGINES[0], false), status(&ENGINES[1], false)];
        assert_eq!(
            choose_engine(&statuses, "claude").unwrap_err(),
            NoEngineAvailable
        );
    }

    const CLAUDE_ENVELOPE: &str = include_str!("../../tests/fixtures/claude_envelope.json");
    const CLAUDE_STRUCTURED: &str =
        include_str!("../../tests/fixtures/claude_envelope_structured.json");
    const CLAUDE_ERROR: &str = include_str!("../../tests/fixtures/claude_envelope_error.json");
    const CURSOR_ENVELOPE: &str = include_str!("../../tests/fixtures/cursor_envelope.json");

    #[test]
    fn claude_strategy_unwraps_the_result_field() {
        let text = envelope_text(ParseStrategy::ClaudeJson, CLAUDE_ENVELOPE).unwrap();
        assert_eq!(text, "{\"summary\":\"inner payload\",\"findings\":[]}");
    }

    #[test]
    fn only_the_claude_strategy_reports_usage() {
        struct Case {
            name: &'static str,
            strategy: ParseStrategy,
            stdout: &'static str,
            want: Option<EngineUsage>,
        }
        let cases = [
            Case {
                name: "claude envelope carries cost and tokens",
                strategy: ParseStrategy::ClaudeJson,
                stdout: CLAUDE_ENVELOPE,
                want: Some(EngineUsage {
                    input_tokens: 18_234,
                    output_tokens: 590,
                    cost_usd: 0.0142,
                }),
            },
            Case {
                name: "a claude envelope without usage reports nothing",
                strategy: ParseStrategy::ClaudeJson,
                stdout: CLAUDE_STRUCTURED,
                want: None,
            },
            Case {
                name: "generic engines report nothing",
                strategy: ParseStrategy::GenericJson,
                stdout: CLAUDE_ENVELOPE,
                want: None,
            },
            Case {
                name: "raw text engines report nothing",
                strategy: ParseStrategy::RawText,
                stdout: CLAUDE_ENVELOPE,
                want: None,
            },
            Case {
                name: "non-json stdout reports nothing",
                strategy: ParseStrategy::ClaudeJson,
                stdout: "just some prose",
                want: None,
            },
        ];
        for case in cases {
            let output = envelope_output(case.strategy, case.stdout).unwrap();
            assert_eq!(output.usage, case.want, "{}", case.name);
        }
    }

    #[test]
    fn claude_strategy_prefers_structured_output() {
        let text = envelope_text(ParseStrategy::ClaudeJson, CLAUDE_STRUCTURED).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["title"], "Structured");
    }

    #[test]
    fn claude_strategy_surfaces_reported_errors() {
        let error = envelope_text(ParseStrategy::ClaudeJson, CLAUDE_ERROR).unwrap_err();
        assert_eq!(
            error,
            EngineError::Reported("Credit balance is too low".to_string())
        );
    }

    #[test]
    fn claude_strategy_passes_non_json_output_through() {
        let text = envelope_text(ParseStrategy::ClaudeJson, "plain chatter").unwrap();
        assert_eq!(text, "plain chatter");
    }

    #[test]
    fn generic_strategy_probes_known_text_keys() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "cursor envelope fixture",
                input: CURSOR_ENVELOPE,
                want: "{\"summary\":\"cursor payload\",\"findings\":[]}",
            },
            Case {
                name: "text key",
                input: "{\"text\": \"payload\"}",
                want: "payload",
            },
            Case {
                name: "message key",
                input: "{\"message\": \"payload\"}",
                want: "payload",
            },
            Case {
                name: "envelope on last line after logs",
                input: "log line one\nlog line two\n{\"result\": \"payload\"}",
                want: "payload",
            },
            Case {
                name: "no known key falls back to raw",
                input: "{\"weird\": true}",
                want: "{\"weird\": true}",
            },
            Case {
                name: "plain text falls back to raw",
                input: "just text",
                want: "just text",
            },
        ];
        for case in cases {
            let text = envelope_text(ParseStrategy::GenericJson, case.input).unwrap();
            assert_eq!(text, case.want, "{}", case.name);
        }
    }

    #[test]
    fn raw_strategy_returns_stdout_unchanged() {
        let text = envelope_text(ParseStrategy::RawText, "anything {\"a\":1}").unwrap();
        assert_eq!(text, "anything {\"a\":1}");
    }
}
