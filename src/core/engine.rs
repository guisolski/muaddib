use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineId {
    Claude,
    CursorAgent,
    Codex,
    Opencode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseStrategy {
    ClaudeJson,
    GenericJson,
    RawText,
}

#[derive(Debug)]
pub struct EngineSpec {
    pub id: EngineId,
    pub name: &'static str,
    pub bin: &'static str,
    pub args: &'static [&'static str],
    pub parse: ParseStrategy,
    pub supports_json_schema: bool,
    pub model_flag: Option<&'static str>,
    pub models: &'static [&'static str],
    pub fast_model: Option<&'static str>,
    pub install_hint: &'static str,
}

pub const ENGINES: &[EngineSpec] = &[
    EngineSpec {
        id: EngineId::Claude,
        name: "claude",
        bin: "claude",
        args: &[
            "-p",
            "--output-format",
            "json",
            "--allowedTools=WebSearch,WebFetch",
        ],
        parse: ParseStrategy::ClaudeJson,
        supports_json_schema: true,
        model_flag: Some("--model"),
        models: &["opus", "sonnet", "haiku"],
        fast_model: Some("haiku"),
        install_hint: "npm install -g @anthropic-ai/claude-code",
    },
    EngineSpec {
        id: EngineId::CursorAgent,
        name: "cursor-agent",
        bin: "cursor-agent",
        args: &["-p", "--output-format", "json"],
        parse: ParseStrategy::GenericJson,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["auto", "gpt-5", "sonnet-4.5"],
        fast_model: None,
        install_hint: "curl https://cursor.com/install -fsS | bash",
    },
    EngineSpec {
        id: EngineId::Codex,
        name: "codex",
        bin: "codex",
        args: &["exec", "--skip-git-repo-check"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["gpt-5-codex", "gpt-5"],
        fast_model: None,
        install_hint: "npm install -g @openai/codex",
    },
    EngineSpec {
        id: EngineId::Opencode,
        name: "opencode",
        bin: "opencode",
        args: &["run"],
        parse: ParseStrategy::RawText,
        supports_json_schema: false,
        model_flag: Some("--model"),
        models: &["anthropic/claude-sonnet-4-5", "openai/gpt-5"],
        fast_model: None,
        install_hint: "npm install -g opencode-ai",
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineOutput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("engine timed out after {0:?}")]
    TimedOut(Duration),
    #[error("engine exited with status {status}: {stderr_tail}")]
    Failed { status: i32, stderr_tail: String },
    #[error("engine reported an error: {0}")]
    Reported(String),
    #[error("failed to spawn engine: {0}")]
    Spawn(String),
}

pub fn build_args(spec: &EngineSpec, model: Option<&str>, job: &EngineJob) -> Vec<String> {
    let mut args: Vec<String> = spec.args.iter().map(ToString::to_string).collect();
    if let (Some(flag), Some(model)) = (spec.model_flag, model) {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no supported engine CLI is installed")]
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
        .find(|status| status.available)
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

fn claude_envelope_text(stdout: &str) -> Result<String, EngineError> {
    let Ok(envelope) = serde_json::from_str::<Value>(stdout.trim()) else {
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
            spec,
            available,
            path: available.then(|| PathBuf::from("/fake/bin")),
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
    fn build_args_places_the_prompt_last() {
        for spec in ENGINES {
            let args = build_args(spec, Some("some-model"), &job("the prompt", None));
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
            let with_model = build_args(spec, Some("some-model"), &job("p", None));
            let without_model = build_args(spec, None, &job("p", None));
            assert!(
                with_model.windows(2).any(|pair| {
                    pair[0] == spec.model_flag.unwrap_or_default() && pair[1] == "some-model"
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
            let args = build_args(spec, None, &job("p", case.schema));
            assert_eq!(
                args.iter().any(|arg| arg == "--json-schema"),
                case.want_flag,
                "{}",
                case.name
            );
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
