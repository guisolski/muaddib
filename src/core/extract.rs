use serde_json::Value;

type Extractor = fn(&str) -> Option<Value>;

const EXTRACTORS: &[Extractor] = &[
    parse_whole_text,
    parse_fenced_blocks,
    parse_balanced_objects,
];

pub fn extract_json(raw: &str) -> Option<Value> {
    EXTRACTORS.iter().find_map(|extract| extract(raw))
}

fn parse_whole_text(raw: &str) -> Option<Value> {
    serde_json::from_str(raw.trim()).ok()
}

fn parse_fenced_blocks(raw: &str) -> Option<Value> {
    fenced_block_contents(raw)
        .into_iter()
        .find_map(|block| serde_json::from_str(block.trim()).ok())
}

fn fenced_block_contents(raw: &str) -> Vec<&str> {
    const FENCE: &str = "```";
    let mut blocks = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find(FENCE) {
        let after_open = &rest[open + FENCE.len()..];
        let content = skip_fence_info_line(after_open);
        let Some(close) = content.find(FENCE) else {
            break;
        };
        blocks.push(&content[..close]);
        rest = &content[close + FENCE.len()..];
    }
    blocks
}

fn skip_fence_info_line(after_open: &str) -> &str {
    after_open
        .find('\n')
        .map_or("", |newline| &after_open[newline + 1..])
}

fn parse_balanced_objects(raw: &str) -> Option<Value> {
    let mut search_from = 0;
    while let Some(offset) = raw[search_from..].find('{') {
        let start = search_from + offset;
        if let Some(end) = balanced_object_end(raw, start)
            && let Ok(value) = serde_json::from_str(&raw[start..end])
        {
            return Some(value);
        }
        search_from = start + 1;
    }
    None
}

fn balanced_object_end(raw: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in raw[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_json_handles_common_cli_output_shapes() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: Option<Value>,
        }
        let cases = [
            Case {
                name: "bare object",
                input: "{\"a\": 1}",
                want: Some(json!({"a": 1})),
            },
            Case {
                name: "bare array",
                input: "[1, 2]",
                want: Some(json!([1, 2])),
            },
            Case {
                name: "surrounding whitespace",
                input: "  \n {\"a\": 1} \n ",
                want: Some(json!({"a": 1})),
            },
            Case {
                name: "json fence with language tag",
                input: "Here you go:\n```json\n{\"a\": 1}\n```\nDone.",
                want: Some(json!({"a": 1})),
            },
            Case {
                name: "fence without language tag",
                input: "```\n{\"a\": 2}\n```",
                want: Some(json!({"a": 2})),
            },
            Case {
                name: "second fence holds the json",
                input: "```text\nnot json\n```\n```json\n{\"a\": 3}\n```",
                want: Some(json!({"a": 3})),
            },
            Case {
                name: "prose wrapped object",
                input: "The result is {\"a\": 1} as requested.",
                want: Some(json!({"a": 1})),
            },
            Case {
                name: "braces inside strings",
                input: "note {\"text\": \"uses { and } freely\"} end",
                want: Some(json!({"text": "uses { and } freely"})),
            },
            Case {
                name: "escaped quotes inside strings",
                input: "{\"text\": \"she said \\\"hi\\\"\"}",
                want: Some(json!({"text": "she said \"hi\""})),
            },
            Case {
                name: "first of multiple objects wins",
                input: "{\"first\": true} and later {\"second\": true}",
                want: Some(json!({"first": true})),
            },
            Case {
                name: "invalid brace run then valid object",
                input: "{oops} but then {\"a\": 4}",
                want: Some(json!({"a": 4})),
            },
            Case {
                name: "nested objects stay whole",
                input: "x {\"outer\": {\"inner\": 1}} y",
                want: Some(json!({"outer": {"inner": 1}})),
            },
            Case {
                name: "unterminated object",
                input: "{\"a\": 1",
                want: None,
            },
            Case {
                name: "plain prose",
                input: "no structured data here",
                want: None,
            },
            Case {
                name: "empty input",
                input: "",
                want: None,
            },
            Case {
                name: "url with slashes never trips scanning",
                input: "see {\"url\": \"https://example.com/a//b\"} ok",
                want: Some(json!({"url": "https://example.com/a//b"})),
            },
        ];
        for case in cases {
            assert_eq!(extract_json(case.input), case.want, "{}", case.name);
        }
    }

    #[test]
    fn fenced_block_contents_collects_every_block() {
        let raw = "a\n```json\nX\n```\nb\n```\nY\n```\nc";
        assert_eq!(fenced_block_contents(raw), vec!["X\n", "Y\n"]);
    }

    #[test]
    fn balanced_object_end_ignores_braces_in_strings() {
        let raw = "{\"a\": \"}\"}";
        assert_eq!(balanced_object_end(raw, 0), Some(raw.len()));
    }
}
