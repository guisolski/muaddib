use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    General,
    Scientific,
    News,
    Code,
    Forums,
    Deep,
    Exhaustive,
}

pub const MAX_REFLECT_ROUNDS: u8 = 1;

#[derive(Debug)]
pub struct ModeSpec {
    pub mode: Mode,
    pub label: &'static str,
    pub breadth: u8,
    pub cross_language: bool,
    pub instructions: &'static str,
    pub facets: &'static [&'static str],
    pub site_hints: &'static [&'static str],
    pub source_notes: bool,
    pub reflect_rounds: u8,
}

pub const MODES: &[ModeSpec] = &[
    ModeSpec {
        mode: Mode::General,
        label: "General",
        breadth: 3,
        cross_language: true,
        instructions: "Provide a balanced overview of the topic backed by reputable sources.",
        facets: &[
            "overview",
            "advantages and disadvantages",
            "recent developments",
            "common criticisms",
            "practical examples",
            "comparison with alternatives",
            "frequently asked questions",
        ],
        site_hints: &[],
        source_notes: false,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::Scientific,
        label: "Scientific",
        breadth: 4,
        cross_language: true,
        instructions: "Prioritize peer-reviewed papers and preprints. Prefer arXiv, DOI, and PubMed links. Cite venue and year for every claim.",
        facets: &[
            "peer-reviewed research",
            "survey papers",
            "methodology",
            "open problems",
            "recent findings",
            "datasets and benchmarks",
            "replication studies",
        ],
        site_hints: &[],
        source_notes: false,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::News,
        label: "News",
        breadth: 3,
        cross_language: true,
        instructions: "Focus on coverage from the last 30 days. Prioritize recency, consult multiple outlets, and attach dates to claims.",
        facets: &[
            "latest news",
            "timeline of events",
            "official statements",
            "analysis and commentary",
            "regional coverage",
            "fact check",
            "economic impact",
        ],
        site_hints: &[],
        source_notes: false,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::Code,
        label: "Code",
        breadth: 3,
        cross_language: false,
        instructions: "Answer from official documentation, source repositories, and accepted answers. Prefer the current stable version, name it, and show minimal runnable snippets over prose.",
        facets: &[
            "official documentation",
            "api reference",
            "common errors and fixes",
            "best practices",
            "performance considerations",
            "migration and breaking changes",
            "real-world usage examples",
        ],
        site_hints: &["site:stackoverflow.com", "site:github.com", "site:docs.rs"],
        source_notes: false,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::Forums,
        label: "Forums",
        breadth: 3,
        cross_language: true,
        instructions: "Gather what practitioners actually report: first-hand experience, recurring complaints, and workarounds. Attribute opinions to the community rather than stating them as fact, and note when a thread is old.",
        facets: &[
            "user experiences",
            "common complaints",
            "recommendations",
            "head-to-head comparisons",
            "workarounds",
            "recent discussions",
            "expert replies",
        ],
        site_hints: &[
            "site:reddit.com",
            "site:news.ycombinator.com",
            "site:lobste.rs",
        ],
        source_notes: false,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::Deep,
        label: "Deep",
        breadth: 6,
        cross_language: true,
        instructions: "Cover the topic exhaustively across distinct facets. Include contrarian views and primary sources.",
        facets: &[
            "history and origins",
            "state of the art",
            "criticism and controversies",
            "alternatives and competitors",
            "future outlook",
            "case studies",
            "expert opinions",
        ],
        site_hints: &[],
        source_notes: true,
        reflect_rounds: 0,
    },
    ModeSpec {
        mode: Mode::Exhaustive,
        label: "Exhaustive",
        breadth: 6,
        cross_language: true,
        instructions: "Leave nothing unexamined. Prefer primary sources over summaries, quantify every claim the evidence allows, and actively hunt for material that contradicts the emerging picture.",
        facets: &[
            "primary sources and raw data",
            "quantitative evidence",
            "contradicting evidence",
            "methodology and limitations",
            "edge cases and exceptions",
            "regional and sector differences",
            "unresolved questions",
        ],
        site_hints: &[],
        source_notes: true,
        reflect_rounds: 1,
    },
];

impl Mode {
    pub fn spec(self) -> &'static ModeSpec {
        MODES
            .iter()
            .find(|spec| spec.mode == self)
            .expect("MODES holds one row per Mode variant")
    }

    pub fn all() -> impl Iterator<Item = Mode> {
        MODES.iter().map(|spec| spec.mode)
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown mode: {0}")]
pub struct UnknownMode(String);

impl FromStr for Mode {
    type Err = UnknownMode;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        MODES
            .iter()
            .find(|spec| spec.label.eq_ignore_ascii_case(input))
            .map(|spec| spec.mode)
            .ok_or_else(|| UnknownMode(input.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::MAX_BREADTH;
    use std::collections::BTreeSet;

    #[test]
    fn modes_table_labels_are_unique() {
        let labels: BTreeSet<&str> = MODES.iter().map(|spec| spec.label).collect();
        assert_eq!(labels.len(), MODES.len());
    }

    #[test]
    fn modes_table_breadth_is_at_least_one() {
        for spec in MODES {
            assert!(spec.breadth >= 1, "{}", spec.label);
        }
    }

    #[test]
    fn modes_table_instructions_are_not_empty() {
        for spec in MODES {
            assert!(!spec.instructions.is_empty(), "{}", spec.label);
        }
    }

    #[test]
    fn modes_table_facets_support_max_breadth() {
        for spec in MODES {
            assert!(
                spec.facets.len() >= usize::from(MAX_BREADTH) - 1,
                "{}",
                spec.label
            );
        }
    }

    #[test]
    fn modes_table_keeps_reflection_within_the_budget_cap() {
        for spec in MODES {
            assert!(spec.reflect_rounds <= MAX_REFLECT_ROUNDS, "{}", spec.label);
        }
    }

    #[test]
    fn reflecting_modes_also_ask_the_model_for_source_notes() {
        for spec in MODES {
            assert!(
                spec.reflect_rounds == 0 || spec.source_notes,
                "{}",
                spec.label
            );
        }
    }

    #[test]
    fn every_mode_resolves_its_own_spec() {
        for mode in Mode::all() {
            assert_eq!(mode.spec().mode, mode);
        }
    }

    #[test]
    fn mode_parses_from_label_case_insensitively() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: Result<Mode, UnknownMode>,
        }
        let cases = [
            Case {
                name: "exact label",
                input: "General",
                want: Ok(Mode::General),
            },
            Case {
                name: "lowercase",
                input: "scientific",
                want: Ok(Mode::Scientific),
            },
            Case {
                name: "uppercase",
                input: "NEWS",
                want: Ok(Mode::News),
            },
            Case {
                name: "mixed case",
                input: "dEEp",
                want: Ok(Mode::Deep),
            },
            Case {
                name: "exhaustive",
                input: "exhaustive",
                want: Ok(Mode::Exhaustive),
            },
            Case {
                name: "unknown",
                input: "casual",
                want: Err(UnknownMode("casual".to_string())),
            },
        ];
        for case in cases {
            assert_eq!(case.input.parse::<Mode>(), case.want, "{}", case.name);
        }
    }

    #[test]
    fn mode_serializes_to_lowercase_name() {
        struct Case {
            name: &'static str,
            mode: Mode,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "general",
                mode: Mode::General,
                want: "\"general\"",
            },
            Case {
                name: "scientific",
                mode: Mode::Scientific,
                want: "\"scientific\"",
            },
            Case {
                name: "news",
                mode: Mode::News,
                want: "\"news\"",
            },
            Case {
                name: "deep",
                mode: Mode::Deep,
                want: "\"deep\"",
            },
            Case {
                name: "exhaustive",
                mode: Mode::Exhaustive,
                want: "\"exhaustive\"",
            },
        ];
        for case in cases {
            let serialized = serde_json::to_string(&case.mode).unwrap();
            assert_eq!(serialized, case.want, "{}", case.name);
        }
    }
}
