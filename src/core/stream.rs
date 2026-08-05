use serde_json::Value;

pub const MAX_TARGET_CHARS: usize = 64;

#[derive(Debug)]
pub struct StreamTool {
    pub tool: &'static str,
    pub label: &'static str,
    pub target_key: &'static str,
}

pub const STREAM_TOOLS: &[StreamTool] = &[
    StreamTool {
        tool: "WebSearch",
        label: "searching",
        target_key: "query",
    },
    StreamTool {
        tool: "WebFetch",
        label: "reading",
        target_key: "url",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineActivity {
    pub label: &'static str,
    pub target: String,
}

pub fn activities_in(line: &str) -> Vec<EngineActivity> {
    let Some(value) = parse_line(line) else {
        return Vec::new();
    };
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return Vec::new();
    }
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(activity_of).collect())
        .unwrap_or_default()
}

pub fn result_line(stdout: &str) -> Option<&str> {
    stdout.lines().rev().find(|line| {
        parse_line(line)
            .is_some_and(|value| value.get("type").and_then(Value::as_str) == Some("result"))
    })
}

fn parse_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    trimmed
        .starts_with('{')
        .then(|| serde_json::from_str(trimmed).ok())
        .flatten()
}

fn activity_of(block: &Value) -> Option<EngineActivity> {
    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return None;
    }
    let name = block.get("name").and_then(Value::as_str)?;
    let spec = STREAM_TOOLS.iter().find(|spec| spec.tool == name)?;
    let target = block
        .get("input")
        .and_then(|input| input.get(spec.target_key))
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(EngineActivity {
        label: spec.label,
        target: excerpt(target, MAX_TARGET_CHARS),
    })
}

fn excerpt(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_line(blocks: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"content":{blocks}}}}}"#)
    }

    #[test]
    fn stream_tools_table_has_no_duplicate_tool_names() {
        let names: std::collections::BTreeSet<&str> =
            STREAM_TOOLS.iter().map(|spec| spec.tool).collect();
        assert_eq!(names.len(), STREAM_TOOLS.len());
        for spec in STREAM_TOOLS {
            assert!(!spec.label.is_empty(), "{}", spec.tool);
            assert!(!spec.target_key.is_empty(), "{}", spec.tool);
        }
    }

    #[test]
    fn only_the_tools_in_the_table_become_activity() {
        struct Case {
            name: &'static str,
            line: String,
            want: Vec<EngineActivity>,
        }
        let cases = [
            Case {
                name: "a web search reports its query",
                line: assistant_line(
                    r#"[{"type":"tool_use","name":"WebSearch","input":{"query":"rust async"}}]"#,
                ),
                want: vec![EngineActivity {
                    label: "searching",
                    target: "rust async".to_string(),
                }],
            },
            Case {
                name: "a page fetch reports its url",
                line: assistant_line(
                    r#"[{"type":"tool_use","name":"WebFetch","input":{"url":"https://a.example","prompt":"ignored"}}]"#,
                ),
                want: vec![EngineActivity {
                    label: "reading",
                    target: "https://a.example".to_string(),
                }],
            },
            Case {
                name: "parallel tool calls each report",
                line: assistant_line(
                    r#"[{"type":"tool_use","name":"WebSearch","input":{"query":"one"}},
                        {"type":"tool_use","name":"WebFetch","input":{"url":"https://two.example"}}]"#,
                ),
                want: vec![
                    EngineActivity {
                        label: "searching",
                        target: "one".to_string(),
                    },
                    EngineActivity {
                        label: "reading",
                        target: "https://two.example".to_string(),
                    },
                ],
            },
            Case {
                name: "the structured output call is not narrated",
                line: assistant_line(
                    r#"[{"type":"tool_use","name":"StructuredOutput","input":{"title":"x"}}]"#,
                ),
                want: Vec::new(),
            },
            Case {
                name: "an unknown tool is not narrated",
                line: assistant_line(
                    r#"[{"type":"tool_use","name":"Bash","input":{"command":"rm -rf /"}}]"#,
                ),
                want: Vec::new(),
            },
            Case {
                name: "thinking blocks are not narrated",
                line: assistant_line(r#"[{"type":"thinking","thinking":"hmm"}]"#),
                want: Vec::new(),
            },
            Case {
                name: "a missing target degrades to an empty one",
                line: assistant_line(r#"[{"type":"tool_use","name":"WebSearch","input":{}}]"#),
                want: vec![EngineActivity {
                    label: "searching",
                    target: String::new(),
                }],
            },
            Case {
                name: "partial message chunks are not narrated",
                line: r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#
                    .to_string(),
                want: Vec::new(),
            },
            Case {
                name: "the result line is not narrated",
                line: r#"{"type":"result","subtype":"success","result":"done"}"#.to_string(),
                want: Vec::new(),
            },
            Case {
                name: "a truncated line degrades silently",
                line: r#"{"type":"assistant","message":{"conte"#.to_string(),
                want: Vec::new(),
            },
            Case {
                name: "plain log noise degrades silently",
                line: "Warning: no stdin data received in 3s".to_string(),
                want: Vec::new(),
            },
            Case {
                name: "an empty line degrades silently",
                line: String::new(),
                want: Vec::new(),
            },
        ];
        for case in cases {
            assert_eq!(activities_in(&case.line), case.want, "{}", case.name);
        }
    }

    #[test]
    fn a_long_target_is_cut_to_one_line() {
        let query = "a".repeat(200);
        let line = assistant_line(&format!(
            r#"[{{"type":"tool_use","name":"WebSearch","input":{{"query":"{query}"}}}}]"#
        ));
        let target = &activities_in(&line)[0].target;
        assert_eq!(target.chars().count(), MAX_TARGET_CHARS);
        assert!(target.ends_with('…'));
    }

    #[test]
    fn the_result_line_is_found_wherever_it_sits() {
        struct Case {
            name: &'static str,
            stdout: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "the last line of a well-formed stream",
                stdout: "{\"type\":\"system\"}\n{\"type\":\"assistant\"}\n{\"type\":\"result\",\"result\":\"x\"}",
                want: Some("{\"type\":\"result\",\"result\":\"x\"}"),
            },
            Case {
                name: "trailing blank lines do not hide it",
                stdout: "{\"type\":\"result\",\"result\":\"x\"}\n\n",
                want: Some("{\"type\":\"result\",\"result\":\"x\"}"),
            },
            Case {
                name: "the last result wins when a stream carries two",
                stdout: "{\"type\":\"result\",\"result\":\"first\"}\n{\"type\":\"result\",\"result\":\"second\"}",
                want: Some("{\"type\":\"result\",\"result\":\"second\"}"),
            },
            Case {
                name: "a stream that never finished has none",
                stdout: "{\"type\":\"system\"}\n{\"type\":\"assistant\"}",
                want: None,
            },
            Case {
                name: "plain text has none",
                stdout: "just some prose",
                want: None,
            },
            Case {
                name: "empty output has none",
                stdout: "",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(result_line(case.stdout), case.want, "{}", case.name);
        }
    }
}
