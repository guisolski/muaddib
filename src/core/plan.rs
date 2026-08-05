use crate::core::config::{MAX_BREADTH, MODE_DEFAULT_BREADTH};
use crate::core::mode::Mode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubQuery {
    pub query: String,
    pub lang: String,
    pub rationale: String,
}

impl Default for SubQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            lang: "en".to_string(),
            rationale: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPlan {
    pub original: String,
    pub mode: Mode,
    pub answer_lang: String,
    pub sub_queries: Vec<SubQuery>,
}

pub fn effective_breadth(mode: Mode, override_breadth: u8) -> u8 {
    if override_breadth == MODE_DEFAULT_BREADTH {
        mode.spec().breadth
    } else {
        override_breadth.clamp(1, MAX_BREADTH)
    }
}

pub const SUB_QUERIES_PER_TIMEOUT_UNIT: usize = 3;
pub const MAX_SYNTHESIS_SCALE: u32 = 3;

pub fn synthesis_scale(sub_queries: usize) -> u32 {
    let units = sub_queries.div_ceil(SUB_QUERIES_PER_TIMEOUT_UNIT).max(1);
    u32::try_from(units)
        .unwrap_or(MAX_SYNTHESIS_SCALE)
        .min(MAX_SYNTHESIS_SCALE)
}

pub fn synthesis_timeout(base: Duration, sub_queries: usize) -> Duration {
    base.saturating_mul(synthesis_scale(sub_queries))
}

pub fn literal_plan(original: &str, mode: Mode, answer_lang: &str) -> SearchPlan {
    SearchPlan {
        original: original.to_string(),
        mode,
        answer_lang: answer_lang.to_string(),
        sub_queries: vec![literal_subquery(original, answer_lang)],
    }
}

pub fn plan_from_expansion(
    original: &str,
    mode: Mode,
    answer_lang: &str,
    expansion: &Value,
    breadth: u8,
) -> SearchPlan {
    let breadth = if simple_complexity(expansion) {
        1
    } else {
        breadth
    };
    let parsed = parse_subqueries(expansion, answer_lang);
    let sub_queries = if parsed.is_empty() {
        fallback_expansion(original, mode, answer_lang, breadth)
    } else {
        with_original_first(original, answer_lang, parsed, breadth)
    };
    SearchPlan {
        original: original.to_string(),
        mode,
        answer_lang: answer_lang.to_string(),
        sub_queries,
    }
}

fn simple_complexity(expansion: &Value) -> bool {
    expansion
        .get("complexity")
        .and_then(Value::as_str)
        .is_some_and(|rating| rating.eq_ignore_ascii_case("simple"))
}

fn parse_subqueries(expansion: &Value, default_lang: &str) -> Vec<SubQuery> {
    expansion
        .get("subqueries")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_subquery(item, default_lang))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_subquery(item: &Value, default_lang: &str) -> Option<SubQuery> {
    let query = non_empty_str(item.get("query")?)?;
    let lang = item
        .get("lang")
        .and_then(non_empty_str)
        .unwrap_or_else(|| default_lang.to_string());
    let rationale = item
        .get("rationale")
        .and_then(non_empty_str)
        .unwrap_or_default();
    Some(SubQuery {
        query,
        lang,
        rationale,
    })
}

fn non_empty_str(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn with_original_first(
    original: &str,
    answer_lang: &str,
    mut parsed: Vec<SubQuery>,
    breadth: u8,
) -> Vec<SubQuery> {
    let already_included = parsed
        .iter()
        .any(|sub| sub.query.eq_ignore_ascii_case(original.trim()));
    if !already_included {
        parsed.insert(0, literal_subquery(original, answer_lang));
    }
    parsed.truncate(usize::from(breadth.max(1)));
    parsed
}

fn literal_subquery(original: &str, answer_lang: &str) -> SubQuery {
    SubQuery {
        query: original.trim().to_string(),
        lang: answer_lang.to_string(),
        rationale: "literal query".to_string(),
    }
}

pub fn fallback_expansion(
    original: &str,
    mode: Mode,
    answer_lang: &str,
    breadth: u8,
) -> Vec<SubQuery> {
    let cross_language = mode.spec().cross_language;
    let facet_lang = if cross_language { "en" } else { answer_lang };
    let facet_count = usize::from(breadth.max(1)) - 1;
    let facets = mode
        .spec()
        .facets
        .iter()
        .take(facet_count)
        .map(|facet| SubQuery {
            query: format!("{} {facet}", original.trim()),
            lang: facet_lang.to_string(),
            rationale: format!("facet: {facet}"),
        });
    std::iter::once(literal_subquery(original, answer_lang))
        .chain(facets)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn synthesis_timeout_grows_with_the_findings_it_has_to_read() {
        struct Case {
            name: &'static str,
            sub_queries: usize,
            want_secs: u64,
        }
        let base = Duration::from_mins(3);
        let cases = [
            Case {
                name: "a simple rating keeps the base budget",
                sub_queries: 1,
                want_secs: 180,
            },
            Case {
                name: "general breadth keeps the base budget",
                sub_queries: 3,
                want_secs: 180,
            },
            Case {
                name: "scientific breadth buys a second unit",
                sub_queries: 4,
                want_secs: 360,
            },
            Case {
                name: "deep breadth buys a second unit",
                sub_queries: 6,
                want_secs: 360,
            },
            Case {
                name: "the maximum breadth buys a third",
                sub_queries: 8,
                want_secs: 540,
            },
            Case {
                name: "the scale is capped so a bad plan cannot hang forever",
                sub_queries: 100,
                want_secs: 540,
            },
            Case {
                name: "an empty plan still gets a budget",
                sub_queries: 0,
                want_secs: 180,
            },
        ];
        for case in cases {
            assert_eq!(
                synthesis_timeout(base, case.sub_queries),
                Duration::from_secs(case.want_secs),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn effective_breadth_uses_mode_default_when_zero() {
        struct Case {
            name: &'static str,
            mode: Mode,
            override_breadth: u8,
            want: u8,
        }
        let cases = [
            Case {
                name: "general default",
                mode: Mode::General,
                override_breadth: 0,
                want: 3,
            },
            Case {
                name: "deep default",
                mode: Mode::Deep,
                override_breadth: 0,
                want: 6,
            },
            Case {
                name: "explicit override",
                mode: Mode::General,
                override_breadth: 5,
                want: 5,
            },
            Case {
                name: "override clamped high",
                mode: Mode::General,
                override_breadth: 200,
                want: MAX_BREADTH,
            },
        ];
        for case in cases {
            assert_eq!(
                effective_breadth(case.mode, case.override_breadth),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn fallback_expansion_always_starts_with_the_literal_query() {
        for mode in Mode::all() {
            for breadth in [1u8, 3, MAX_BREADTH] {
                let subs = fallback_expansion("rust async", mode, "pt-BR", breadth);
                assert_eq!(subs[0].query, "rust async", "{mode} breadth {breadth}");
                assert_eq!(subs[0].lang, "pt-BR", "{mode} breadth {breadth}");
                assert_eq!(subs.len(), usize::from(breadth), "{mode} breadth {breadth}");
            }
        }
    }

    #[test]
    fn fallback_expansion_facets_use_english_for_cross_language_modes() {
        let subs = fallback_expansion("energia solar", Mode::General, "pt-BR", 3);
        assert_eq!(subs[1].lang, "en");
        assert_eq!(subs[2].lang, "en");
        assert!(subs[1].query.starts_with("energia solar "));
    }

    #[test]
    fn plan_from_expansion_parses_valid_subqueries() {
        let expansion = json!({
            "subqueries": [
                {"query": "solar energy overview", "lang": "en", "rationale": "facet"},
                {"query": "energía solar avances", "lang": "es"}
            ]
        });
        let plan = plan_from_expansion("energia solar", Mode::General, "pt-BR", &expansion, 4);
        assert_eq!(plan.sub_queries.len(), 3);
        assert_eq!(plan.sub_queries[0].query, "energia solar");
        assert_eq!(plan.sub_queries[1].query, "solar energy overview");
        assert_eq!(plan.sub_queries[2].lang, "es");
    }

    #[test]
    fn plan_from_expansion_truncates_to_breadth() {
        let expansion = json!({
            "subqueries": [
                {"query": "a"}, {"query": "b"}, {"query": "c"}, {"query": "d"}, {"query": "e"}
            ]
        });
        let plan = plan_from_expansion("root", Mode::General, "en", &expansion, 3);
        assert_eq!(plan.sub_queries.len(), 3);
        assert_eq!(plan.sub_queries[0].query, "root");
    }

    #[test]
    fn plan_from_expansion_skips_malformed_rows() {
        let expansion = json!({
            "subqueries": [
                {"query": ""},
                {"lang": "en"},
                {"query": "  valid one  ", "lang": ""},
                "not an object"
            ]
        });
        let plan = plan_from_expansion("root", Mode::General, "pt-BR", &expansion, 5);
        assert_eq!(plan.sub_queries.len(), 2);
        assert_eq!(plan.sub_queries[1].query, "valid one");
        assert_eq!(plan.sub_queries[1].lang, "pt-BR");
    }

    #[test]
    fn plan_from_expansion_falls_back_when_expansion_is_unusable() {
        struct Case {
            name: &'static str,
            expansion: Value,
        }
        let cases = [
            Case {
                name: "empty object",
                expansion: json!({}),
            },
            Case {
                name: "empty list",
                expansion: json!({"subqueries": []}),
            },
            Case {
                name: "wrong shape",
                expansion: json!({"subqueries": "nope"}),
            },
        ];
        for case in cases {
            let plan = plan_from_expansion("root", Mode::News, "en", &case.expansion, 3);
            assert_eq!(plan.sub_queries.len(), 3, "{}", case.name);
            assert_eq!(plan.sub_queries[0].query, "root", "{}", case.name);
        }
    }

    #[test]
    fn simple_complexity_narrows_the_plan_to_one_subquery() {
        struct Case {
            name: &'static str,
            expansion: Value,
            want_len: usize,
        }
        let cases = [
            Case {
                name: "simple keeps only the literal query",
                expansion: json!({
                    "complexity": "simple",
                    "subqueries": [{"query": "root"}, {"query": "root basics"}]
                }),
                want_len: 1,
            },
            Case {
                name: "rating is case insensitive",
                expansion: json!({
                    "complexity": "Simple",
                    "subqueries": [{"query": "a"}, {"query": "b"}]
                }),
                want_len: 1,
            },
            Case {
                name: "standard keeps the full breadth",
                expansion: json!({
                    "complexity": "standard",
                    "subqueries": [{"query": "a"}, {"query": "b"}]
                }),
                want_len: 3,
            },
            Case {
                name: "missing rating keeps the full breadth",
                expansion: json!({
                    "subqueries": [{"query": "a"}, {"query": "b"}]
                }),
                want_len: 3,
            },
            Case {
                name: "garbage rating keeps the full breadth",
                expansion: json!({
                    "complexity": 7,
                    "subqueries": [{"query": "a"}, {"query": "b"}]
                }),
                want_len: 3,
            },
            Case {
                name: "simple with unusable subqueries falls back to the literal query",
                expansion: json!({"complexity": "simple", "subqueries": []}),
                want_len: 1,
            },
        ];
        for case in cases {
            let plan = plan_from_expansion("root", Mode::General, "en", &case.expansion, 3);
            assert_eq!(plan.sub_queries.len(), case.want_len, "{}", case.name);
            assert_eq!(plan.sub_queries[0].query, "root", "{}", case.name);
        }
    }

    #[test]
    fn plan_keeps_original_when_model_already_included_it() {
        let expansion = json!({
            "subqueries": [
                {"query": "Root", "lang": "en"},
                {"query": "root basics", "lang": "en"}
            ]
        });
        let plan = plan_from_expansion("root", Mode::General, "en", &expansion, 4);
        assert_eq!(plan.sub_queries.len(), 2);
        assert_eq!(plan.sub_queries[0].query, "Root");
    }
}
