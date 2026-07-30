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
    },
    Paragraph {
        text: String,
        #[serde(default)]
        source_ids: Vec<u32>,
    },
    List {
        #[serde(default)]
        ordered: bool,
        items: Vec<ListItem>,
    },
    Quote {
        text: String,
        #[serde(default)]
        source_ids: Vec<u32>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        #[serde(default)]
        source_ids: Vec<u32>,
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
    },
    #[serde(other)]
    Unknown,
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
            "text": {"type": "string"}
          }
        },
        {
          "type": "object",
          "required": ["type", "text", "source_ids"],
          "properties": {
            "type": {"const": "paragraph"},
            "text": {"type": "string"},
            "source_ids": {"$ref": "#/definitions/source_ids"}
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
            }
          }
        },
        {
          "type": "object",
          "required": ["type", "text", "source_ids"],
          "properties": {
            "type": {"const": "quote"},
            "text": {"type": "string"},
            "source_ids": {"$ref": "#/definitions/source_ids"}
          }
        },
        {
          "type": "object",
          "required": ["type", "headers", "rows", "source_ids"],
          "properties": {
            "type": {"const": "table"},
            "headers": {"type": "array", "items": {"type": "string"}},
            "rows": {"type": "array", "items": {"type": "array", "items": {"type": "string"}}},
            "source_ids": {"$ref": "#/definitions/source_ids"}
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
            "source_ids": {"$ref": "#/definitions/source_ids"}
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
                },
                Block::Paragraph {
                    text: "A cited claim.".to_string(),
                    source_ids: vec![1],
                },
                Block::Chart {
                    chart_type: ChartType::Bar,
                    title: "Share".to_string(),
                    labels: vec!["A".to_string(), "B".to_string()],
                    values: vec![40.0, 60.0],
                    unit: "%".to_string(),
                    source_ids: vec![1],
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
                },
            },
            Case {
                name: "heading without level",
                input: json!({"type": "heading", "text": "h"}),
                want: Block::Heading {
                    level: 2,
                    text: "h".to_string(),
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
                },
            },
        ];
        for case in cases {
            let block: Block = serde_json::from_value(case.input.clone()).unwrap();
            assert_eq!(block, case.want, "{}", case.name);
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
