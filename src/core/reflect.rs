use crate::core::plan::{SubQuery, parse_subquery, synthesis_timeout};
use serde_json::Value;
use std::time::Duration;

pub const MAX_REFLECTION_GAPS: usize = 3;

pub fn reflection_timeout(base: Duration, sub_queries: usize) -> Duration {
    synthesis_timeout(base, sub_queries)
        .saturating_add(base)
        .saturating_add(synthesis_timeout(base, sub_queries + MAX_REFLECTION_GAPS))
}

pub fn gaps_from_reflection(value: &Value, done: &[SubQuery], default_lang: &str) -> Vec<SubQuery> {
    let mut seen: Vec<String> = done.iter().map(|sub| normalized(&sub.query)).collect();
    let mut gaps = Vec::new();
    for item in reflection_items(value) {
        if gaps.len() == MAX_REFLECTION_GAPS {
            break;
        }
        let Some(gap) = parse_subquery(item, default_lang) else {
            continue;
        };
        let key = normalized(&gap.query);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        gaps.push(gap);
    }
    gaps
}

fn reflection_items(value: &Value) -> &[Value] {
    value
        .get("gaps")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

fn normalized(query: &str) -> String {
    query.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn done(queries: &[&str]) -> Vec<SubQuery> {
        queries
            .iter()
            .map(|query| SubQuery {
                query: (*query).to_string(),
                ..SubQuery::default()
            })
            .collect()
    }

    fn queries_of(gaps: &[SubQuery]) -> Vec<&str> {
        gaps.iter().map(|gap| gap.query.as_str()).collect()
    }

    #[test]
    fn reflection_budget_covers_a_critique_a_fan_out_and_a_resynthesis() {
        struct Case {
            name: &'static str,
            sub_queries: usize,
            want_secs: u64,
        }
        let base = Duration::from_secs(180);
        let cases = [
            Case {
                name: "a single sub-query still buys all three steps",
                sub_queries: 1,
                want_secs: 180 + 180 + 360,
            },
            Case {
                name: "exhaustive breadth pays for a scaled resynthesis",
                sub_queries: 6,
                want_secs: 360 + 180 + 540,
            },
            Case {
                name: "the scale cap keeps a runaway plan bounded",
                sub_queries: 100,
                want_secs: 540 + 180 + 540,
            },
        ];
        for case in cases {
            assert_eq!(
                reflection_timeout(base, case.sub_queries),
                Duration::from_secs(case.want_secs),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn gaps_are_parsed_validated_capped_and_deduplicated() {
        struct Case {
            name: &'static str,
            value: Value,
            done: Vec<SubQuery>,
            want: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "well-formed gaps come through in order",
                value: json!({"gaps": [
                    {"query": "capacity by region", "lang": "en", "rationale": "no regional data"},
                    {"query": "custo por MWh", "lang": "pt-BR", "rationale": "sem números"}
                ]}),
                done: done(&["solar capacity"]),
                want: vec!["capacity by region", "custo por MWh"],
            },
            Case {
                name: "a gap already searched is dropped whatever its casing",
                value: json!({"gaps": [
                    {"query": "  Solar Capacity  "},
                    {"query": "capacity by region"}
                ]}),
                done: done(&["solar capacity"]),
                want: vec!["capacity by region"],
            },
            Case {
                name: "gaps repeating each other are collapsed",
                value: json!({"gaps": [
                    {"query": "grid losses"},
                    {"query": "GRID LOSSES"}
                ]}),
                done: Vec::new(),
                want: vec!["grid losses"],
            },
            Case {
                name: "the cap holds even when the critic is greedy",
                value: json!({"gaps": [
                    {"query": "one"}, {"query": "two"}, {"query": "three"}, {"query": "four"}
                ]}),
                done: Vec::new(),
                want: vec!["one", "two", "three"],
            },
            Case {
                name: "malformed rows are skipped without losing the good ones",
                value: json!({"gaps": [
                    {"query": ""},
                    {"rationale": "no query"},
                    "not an object",
                    {"query": "  usable  "}
                ]}),
                done: Vec::new(),
                want: vec!["usable"],
            },
            Case {
                name: "an empty gap list means nothing to do",
                value: json!({"gaps": []}),
                done: Vec::new(),
                want: Vec::new(),
            },
            Case {
                name: "a missing gaps key means nothing to do",
                value: json!({}),
                done: Vec::new(),
                want: Vec::new(),
            },
            Case {
                name: "a wrongly shaped gaps key means nothing to do",
                value: json!({"gaps": "nope"}),
                done: Vec::new(),
                want: Vec::new(),
            },
            Case {
                name: "null degrades to nothing to do",
                value: Value::Null,
                done: Vec::new(),
                want: Vec::new(),
            },
        ];
        for case in cases {
            let gaps = gaps_from_reflection(&case.value, &case.done, "en");
            assert_eq!(queries_of(&gaps), case.want, "{}", case.name);
        }
    }

    #[test]
    fn a_gap_without_a_language_inherits_the_answer_language() {
        let value = json!({"gaps": [{"query": "custo real"}, {"query": "raw data", "lang": "en"}]});
        let gaps = gaps_from_reflection(&value, &[], "pt-BR");
        assert_eq!(gaps[0].lang, "pt-BR");
        assert_eq!(gaps[1].lang, "en");
    }
}
