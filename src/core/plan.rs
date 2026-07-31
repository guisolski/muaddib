use crate::core::config::{MAX_BREADTH, MODE_DEFAULT_BREADTH};
use crate::core::mode::Mode;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubQuery {
    pub query: String,
    pub lang: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

fn parse_subquery(item: &Value, default_lang: &str) -> Option<SubQuery> {
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
    let mut sub_queries = vec![literal_subquery(original, answer_lang)];
    let facet_count = usize::from(breadth.max(1)) - 1;
    for facet in fallback_facets(mode).iter().take(facet_count) {
        sub_queries.push(SubQuery {
            query: format!("{} {facet}", original.trim()),
            lang: facet_lang.to_string(),
            rationale: format!("facet: {facet}"),
        });
    }
    sub_queries
}

fn fallback_facets(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::General => &[
            "overview",
            "advantages and disadvantages",
            "recent developments",
            "common criticisms",
            "practical examples",
            "comparison with alternatives",
            "frequently asked questions",
        ],
        Mode::Scientific => &[
            "peer-reviewed research",
            "survey papers",
            "methodology",
            "open problems",
            "recent findings",
            "datasets and benchmarks",
            "replication studies",
        ],
        Mode::News => &[
            "latest news",
            "timeline of events",
            "official statements",
            "analysis and commentary",
            "regional coverage",
            "fact check",
            "economic impact",
        ],
        Mode::Deep => &[
            "history and origins",
            "state of the art",
            "criticism and controversies",
            "alternatives and competitors",
            "future outlook",
            "case studies",
            "expert opinions",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn fallback_facet_tables_support_max_breadth() {
        for mode in Mode::all() {
            assert!(
                fallback_facets(mode).len() >= usize::from(MAX_BREADTH) - 1,
                "{mode}"
            );
        }
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
