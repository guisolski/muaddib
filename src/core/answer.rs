use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Answer {
    pub title: String,
    pub language: String,
    pub blocks: Vec<Block>,
    pub sources: Vec<Source>,
    pub followups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading {
        #[serde(default = "default_heading_level")]
        level: u8,
        text: String,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Paragraph {
        text: String,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    List {
        #[serde(default)]
        ordered: bool,
        items: Vec<ListItem>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Quote {
        text: String,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Chart {
        #[serde(default)]
        chart_type: ChartType,
        title: String,
        labels: Vec<String>,
        values: Vec<f64>,
        #[serde(default)]
        unit: String,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Diagram {
        #[serde(default)]
        diagram_type: DiagramType,
        title: String,
        items: Vec<DiagramItem>,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    Image {
        url: String,
        #[serde(default)]
        caption: String,
        #[serde(default)]
        source_ids: Vec<u32>,
        #[serde(default)]
        emphasis: Emphasis,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum DiagramType {
    #[default]
    Flow,
    Timeline,
}

impl From<String> for DiagramType {
    fn from(raw: String) -> Self {
        if raw.eq_ignore_ascii_case("timeline") {
            Self::Timeline
        } else {
            Self::Flow
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagramItem {
    pub label: String,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum Emphasis {
    #[default]
    None,
    Highlight,
}

impl From<String> for Emphasis {
    fn from(raw: String) -> Self {
        if raw.eq_ignore_ascii_case("highlight") {
            Self::Highlight
        } else {
            Self::None
        }
    }
}

fn default_heading_level() -> u8 {
    2
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum ChartType {
    #[default]
    Bar,
    Other,
}

impl From<String> for ChartType {
    fn from(raw: String) -> Self {
        if raw.eq_ignore_ascii_case("bar") {
            Self::Bar
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub text: String,
    #[serde(default)]
    pub source_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: u32,
    #[serde(default)]
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub lang: String,
}

pub fn parse_answer(value: serde_json::Value) -> Result<Answer, serde_json::Error> {
    serde_json::from_value(value)
}

pub const ANSWER_SCHEMA: &str = r##"{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["title", "language", "blocks", "sources"],
  "properties": {
    "title": {"type": "string"},
    "language": {"type": "string"},
    "blocks": {"type": "array", "items": {"$ref": "#/definitions/block"}},
    "sources": {"type": "array", "items": {"$ref": "#/definitions/source"}},
    "followups": {"type": "array", "items": {"type": "string"}, "maxItems": 3}
  },
  "definitions": {
    "source_ids": {"type": "array", "items": {"type": "integer", "minimum": 1}},
    "emphasis": {"enum": ["none", "highlight"]},
    "source": {
      "type": "object",
      "required": ["id", "title", "url"],
      "properties": {
        "id": {"type": "integer", "minimum": 1},
        "title": {"type": "string"},
        "url": {"type": "string"},
        "lang": {"type": "string"}
      }
    },
    "block": {
      "oneOf": [
        {
          "type": "object",
          "required": ["type", "text"],
          "properties": {
            "type": {"const": "heading"},
            "level": {"type": "integer", "minimum": 1, "maximum": 3},
            "text": {"type": "string"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "text", "source_ids"],
          "properties": {
            "type": {"const": "paragraph"},
            "text": {"type": "string"},
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "items"],
          "properties": {
            "type": {"const": "list"},
            "ordered": {"type": "boolean"},
            "items": {
              "type": "array",
              "items": {
                "type": "object",
                "required": ["text", "source_ids"],
                "properties": {
                  "text": {"type": "string"},
                  "source_ids": {"$ref": "#/definitions/source_ids"}
                }
              }
            },
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "text", "source_ids"],
          "properties": {
            "type": {"const": "quote"},
            "text": {"type": "string"},
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "headers", "rows", "source_ids"],
          "properties": {
            "type": {"const": "table"},
            "headers": {"type": "array", "items": {"type": "string"}},
            "rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}},
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "title", "labels", "values", "source_ids"],
          "properties": {
            "type": {"const": "chart"},
            "chart_type": {"enum": ["bar"]},
            "title": {"type": "string"},
            "unit": {"type": "string"},
            "labels": {"type": "array", "items": {"type": "string"}},
            "values": {"type": "array", "items": {"type": "number"}},
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "title", "items", "source_ids"],
          "properties": {
            "type": {"const": "diagram"},
            "diagram_type": {"enum": ["flow", "timeline"]},
            "title": {"type": "string"},
            "items": {
              "type": "array",
              "items": {
                "type": "object",
                "required": ["label"],
                "properties": {
                  "label": {"type": "string"},
                  "detail": {"type": "string"}
                }
              }
            },
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        },
        {
          "type": "object",
          "required": ["type", "url", "source_ids"],
          "properties": {
            "type": {"const": "image"},
            "url": {"type": "string"},
            "caption": {"type": "string"},
            "source_ids": {"$ref": "#/definitions/source_ids"},
            "emphasis": {"$ref": "#/definitions/emphasis"}
          }
        }
      ]
    }
  }
}"##;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_answer() -> Answer {
        Answer {
            title: "Sample".to_string(),
            language: "en".to_string(),
            blocks: vec![
                Block::Heading {
                    level: 2,
                    text: "Intro".to_string(),
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "A cited claim.".to_string(),
                    source_ids: vec![1],
                    emphasis: Emphasis::Highlight,
                },
                Block::Chart {
                    chart_type: ChartType::Bar,
                    title: "Share".to_string(),
                    labels: vec!["A".to_string(), "B".to_string()],
                    values: vec![40.0, 60.0],
                    unit: "%".to_string(),
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                Block::Diagram {
                    diagram_type: DiagramType::Flow,
                    title: "Pipeline".to_string(),
                    items: vec![DiagramItem {
                        label: "Expand".to_string(),
                        detail: "one engine call".to_string(),
                    }],
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                Block::Image {
                    url: "https://example.com/figure.png".to_string(),
                    caption: "Figure".to_string(),
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
            ],
            sources: vec![Source {
                id: 1,
                title: "Example".to_string(),
                url: "https://example.com".to_string(),
                lang: "en".to_string(),
            }],
            followups: vec!["related query".to_string()],
        }
    }

    #[test]
    fn answer_round_trips_through_json() {
        let answer = sample_answer();
        let text = serde_json::to_string(&answer).unwrap();
        let back: Answer = serde_json::from_str(&text).unwrap();
        assert_eq!(back, answer);
    }

    #[test]
    fn unknown_block_types_deserialize_as_unknown() {
        let value = json!({
            "blocks": [
                {"type": "hologram", "text": "future"},
                {"type": "paragraph", "text": "kept", "source_ids": [1]}
            ]
        });
        let answer = parse_answer(value).unwrap();
        assert_eq!(answer.blocks.len(), 2);
        assert_eq!(answer.blocks[0], Block::Unknown);
        assert_eq!(
            answer.blocks[1],
            Block::Paragraph {
                text: "kept".to_string(),
                source_ids: vec![1],
                emphasis: Emphasis::None,
            }
        );
    }

    #[test]
    fn missing_optional_fields_fall_back_to_defaults() {
        struct Case {
            name: &'static str,
            input: serde_json::Value,
            want: Block,
        }
        let cases = [
            Case {
                name: "paragraph without source_ids",
                input: json!({"type": "paragraph", "text": "t"}),
                want: Block::Paragraph {
                    text: "t".to_string(),
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
            },
            Case {
                name: "heading without level",
                input: json!({"type": "heading", "text": "h"}),
                want: Block::Heading {
                    level: 2,
                    text: "h".to_string(),
                    emphasis: Emphasis::None,
                },
            },
            Case {
                name: "chart with unknown chart_type",
                input: json!({
                    "type": "chart",
                    "chart_type": "pie",
                    "title": "c",
                    "labels": [],
                    "values": []
                }),
                want: Block::Chart {
                    chart_type: ChartType::Other,
                    title: "c".to_string(),
                    labels: vec![],
                    values: vec![],
                    unit: String::new(),
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
            },
        ];
        for case in cases {
            let block: Block = serde_json::from_value(case.input.clone()).unwrap();
            assert_eq!(block, case.want, "{}", case.name);
        }
    }

    #[test]
    fn emphasis_defaults_to_none_and_tolerates_unknown_values() {
        struct Case {
            name: &'static str,
            input: serde_json::Value,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "absent emphasis defaults to none",
                input: json!({"type": "paragraph", "text": "t"}),
                want: "none",
            },
            Case {
                name: "highlight is preserved",
                input: json!({"type": "paragraph", "text": "t", "emphasis": "highlight"}),
                want: "highlight",
            },
            Case {
                name: "unknown emphasis degrades to none",
                input: json!({"type": "paragraph", "text": "t", "emphasis": "sparkle"}),
                want: "none",
            },
        ];
        for case in cases {
            let block: Block = serde_json::from_value(case.input.clone()).unwrap();
            let back = serde_json::to_value(&block).unwrap();
            assert_eq!(back["emphasis"], json!(case.want), "{}", case.name);
        }
    }

    #[test]
    fn diagram_blocks_parse_with_tolerant_fields() {
        struct Case {
            name: &'static str,
            input: serde_json::Value,
            want_type: DiagramType,
            want_detail: &'static str,
        }
        let cases = [
            Case {
                name: "explicit flow",
                input: json!({
                    "type": "diagram",
                    "diagram_type": "flow",
                    "title": "d",
                    "items": [{"label": "a", "detail": "why"}]
                }),
                want_type: DiagramType::Flow,
                want_detail: "why",
            },
            Case {
                name: "timeline",
                input: json!({
                    "type": "diagram",
                    "diagram_type": "timeline",
                    "title": "d",
                    "items": [{"label": "1969", "detail": "moon landing"}]
                }),
                want_type: DiagramType::Timeline,
                want_detail: "moon landing",
            },
            Case {
                name: "missing diagram_type defaults to flow",
                input: json!({
                    "type": "diagram",
                    "title": "d",
                    "items": [{"label": "a"}]
                }),
                want_type: DiagramType::Flow,
                want_detail: "",
            },
            Case {
                name: "unknown diagram_type degrades to flow",
                input: json!({
                    "type": "diagram",
                    "diagram_type": "mindmap",
                    "title": "d",
                    "items": [{"label": "a"}]
                }),
                want_type: DiagramType::Flow,
                want_detail: "",
            },
        ];
        for case in cases {
            let block: Block = serde_json::from_value(case.input.clone()).unwrap();
            let Block::Diagram {
                diagram_type,
                items,
                ..
            } = block
            else {
                panic!("{}", case.name);
            };
            assert_eq!(diagram_type, case.want_type, "{}", case.name);
            assert_eq!(items[0].detail, case.want_detail, "{}", case.name);
        }
    }

    #[test]
    fn image_blocks_parse_with_tolerant_fields() {
        struct Case {
            name: &'static str,
            input: serde_json::Value,
            want_caption: &'static str,
            want_ids: &'static [u32],
        }
        let cases = [
            Case {
                name: "full image block",
                input: json!({
                    "type": "image",
                    "url": "https://img.example/chart.png",
                    "caption": "Installed capacity",
                    "source_ids": [2]
                }),
                want_caption: "Installed capacity",
                want_ids: &[2],
            },
            Case {
                name: "caption and source_ids default",
                input: json!({"type": "image", "url": "https://img.example/photo.jpg"}),
                want_caption: "",
                want_ids: &[],
            },
        ];
        for case in cases {
            let block: Block = serde_json::from_value(case.input.clone()).unwrap();
            let Block::Image {
                url,
                caption,
                source_ids,
                ..
            } = block
            else {
                panic!("{}", case.name);
            };
            assert!(url.starts_with("https://img.example/"), "{}", case.name);
            assert_eq!(caption, case.want_caption, "{}", case.name);
            assert_eq!(source_ids, case.want_ids, "{}", case.name);
        }
    }

    #[test]
    fn schema_declares_emphasis_on_every_block_variant() {
        let schema: serde_json::Value = serde_json::from_str(ANSWER_SCHEMA).unwrap();
        let variants = schema["definitions"]["block"]["oneOf"].as_array().unwrap();
        for variant in variants {
            let block_type = variant["properties"]["type"]["const"].as_str().unwrap();
            assert!(
                variant["properties"]["emphasis"].is_object(),
                "{block_type}"
            );
            let required = variant["required"].as_array().unwrap();
            assert!(
                !required.iter().any(|entry| entry == "emphasis"),
                "{block_type}"
            );
        }
    }

    #[test]
    fn answer_schema_constant_is_valid_json() {
        let schema: serde_json::Value = serde_json::from_str(ANSWER_SCHEMA).unwrap();
        assert_eq!(schema["type"], "object");
        assert!(schema["definitions"]["block"]["oneOf"].is_array());
    }

    #[test]
    fn sample_answer_uses_only_block_types_declared_in_schema() {
        let schema: serde_json::Value = serde_json::from_str(ANSWER_SCHEMA).unwrap();
        let declared: Vec<String> = schema["definitions"]["block"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variant| {
                variant["properties"]["type"]["const"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        let serialized = serde_json::to_value(sample_answer()).unwrap();
        for block in serialized["blocks"].as_array().unwrap() {
            let block_type = block["type"].as_str().unwrap().to_string();
            assert!(declared.contains(&block_type), "{block_type}");
        }
    }
}
