use crate::engines::EngineError;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseStrategy {
    ClaudeJson,
    GenericJson,
    RawText,
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
