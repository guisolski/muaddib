use serde_json::Value;

const INPUT_TOKEN_KEYS: &[&str] = &[
    "input_tokens",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
];

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EngineUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

impl EngineUsage {
    pub fn is_empty(self) -> bool {
        self.input_tokens == 0 && self.output_tokens == 0 && self.cost_usd == 0.0
    }

    pub fn total_tokens(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cost_usd: self.cost_usd + other.cost_usd,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPrice {
    pub prefix: &'static str,
    pub input_per_million: f64,
    pub output_per_million: f64,
}

pub fn price_for<'a>(prices: &'a [ModelPrice], model: &str) -> Option<&'a ModelPrice> {
    prices
        .iter()
        .filter(|price| model.starts_with(price.prefix))
        .max_by_key(|price| price.prefix.len())
}

pub fn estimate_cost(prices: &[ModelPrice], model: &str, usage: EngineUsage) -> f64 {
    price_for(prices, model).map_or(0.0, |price| {
        (usage.input_tokens as f64).mul_add(
            price.input_per_million,
            usage.output_tokens as f64 * price.output_per_million,
        ) / 1_000_000.0
    })
}

pub fn parse_usage(envelope: &Value) -> Option<EngineUsage> {
    let cost = envelope
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let tokens = envelope.get("usage");
    let input_tokens = tokens.map_or(0, |usage| {
        INPUT_TOKEN_KEYS
            .iter()
            .filter_map(|key| usage.get(*key).and_then(Value::as_u64))
            .sum()
    });
    let output_tokens = tokens
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let usage = EngineUsage {
        input_tokens,
        output_tokens,
        cost_usd: cost,
    };
    (!usage.is_empty()).then_some(usage)
}

pub fn format_tokens(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=999_999 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

pub fn format_cost(cost: f64) -> String {
    if cost > 0.0 && cost < 0.1 {
        format!("${cost:.4}")
    } else {
        format!("${cost:.2}")
    }
}

pub fn usage_label(usage: EngineUsage) -> Option<String> {
    if usage.is_empty() {
        return None;
    }
    let tokens = format_tokens(usage.total_tokens());
    if usage.cost_usd > 0.0 {
        Some(format!(
            "{} \u{b7} {tokens} tok",
            format_cost(usage.cost_usd)
        ))
    } else {
        Some(format!("{tokens} tok"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PRICES: &[ModelPrice] = &[
        ModelPrice {
            prefix: "gpt-5-mini",
            input_per_million: 0.25,
            output_per_million: 2.00,
        },
        ModelPrice {
            prefix: "gpt-5",
            input_per_million: 1.25,
            output_per_million: 10.00,
        },
    ];

    #[test]
    fn the_longest_matching_prefix_wins() {
        struct Case {
            name: &'static str,
            model: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "an exact match",
                model: "gpt-5",
                want: Some("gpt-5"),
            },
            Case {
                name: "the more specific row beats the shorter one",
                model: "gpt-5-mini",
                want: Some("gpt-5-mini"),
            },
            Case {
                name: "a dated snapshot still matches its family",
                model: "gpt-5-2026-03-01",
                want: Some("gpt-5"),
            },
            Case {
                name: "an unpriced model has no row",
                model: "qwen3:8b",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                price_for(PRICES, case.model).map(|price| price.prefix),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn cost_is_estimated_per_million_tokens() {
        struct Case {
            name: &'static str,
            model: &'static str,
            usage: EngineUsage,
            want: f64,
        }
        let cases = [
            Case {
                name: "a priced model",
                model: "gpt-5",
                usage: EngineUsage {
                    input_tokens: 1_000_000,
                    output_tokens: 1_000_000,
                    cost_usd: 0.0,
                },
                want: 11.25,
            },
            Case {
                name: "a fraction of a million",
                model: "gpt-5-mini",
                usage: EngineUsage {
                    input_tokens: 100_000,
                    output_tokens: 10_000,
                    cost_usd: 0.0,
                },
                want: 0.045,
            },
            Case {
                name: "a local model costs nothing",
                model: "qwen3:8b",
                usage: EngineUsage {
                    input_tokens: 1_000_000,
                    output_tokens: 1_000_000,
                    cost_usd: 0.0,
                },
                want: 0.0,
            },
            Case {
                name: "no tokens, no cost",
                model: "gpt-5",
                usage: EngineUsage::default(),
                want: 0.0,
            },
        ];
        for case in cases {
            let got = estimate_cost(PRICES, case.model, case.usage);
            assert!(
                (got - case.want).abs() < 1e-9,
                "{}: got {got}, want {}",
                case.name,
                case.want
            );
        }
    }

    #[test]
    fn an_empty_price_table_never_invents_a_cost() {
        let cost = estimate_cost(
            &[],
            "gpt-5",
            EngineUsage {
                input_tokens: 1_000_000,
                output_tokens: 1_000_000,
                cost_usd: 0.0,
            },
        );
        assert!(cost.abs() < f64::EPSILON, "{cost}");
    }

    #[test]
    fn usage_comes_from_the_envelope_including_cache_tokens() {
        struct Case {
            name: &'static str,
            envelope: Value,
            want: Option<EngineUsage>,
        }
        let cases = [
            Case {
                name: "cache tokens count as input",
                envelope: json!({
                    "total_cost_usd": 0.0142,
                    "usage": {
                        "input_tokens": 12,
                        "cache_creation_input_tokens": 400,
                        "cache_read_input_tokens": 17_822,
                        "output_tokens": 590
                    }
                }),
                want: Some(EngineUsage {
                    input_tokens: 18_234,
                    output_tokens: 590,
                    cost_usd: 0.0142,
                }),
            },
            Case {
                name: "cost without tokens still counts",
                envelope: json!({"total_cost_usd": 0.5}),
                want: Some(EngineUsage {
                    input_tokens: 0,
                    output_tokens: 0,
                    cost_usd: 0.5,
                }),
            },
            Case {
                name: "tokens without cost still count",
                envelope: json!({"usage": {"input_tokens": 10, "output_tokens": 20}}),
                want: Some(EngineUsage {
                    input_tokens: 10,
                    output_tokens: 20,
                    cost_usd: 0.0,
                }),
            },
            Case {
                name: "an envelope that reports nothing yields nothing",
                envelope: json!({"type": "result", "is_error": false}),
                want: None,
            },
            Case {
                name: "non-numeric fields are ignored",
                envelope: json!({"total_cost_usd": "free", "usage": {"output_tokens": "many"}}),
                want: None,
            },
            Case {
                name: "a zero-cost envelope reports nothing",
                envelope: json!({"total_cost_usd": 0.0}),
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(parse_usage(&case.envelope), case.want, "{}", case.name);
        }
    }

    #[test]
    fn usage_adds_up_across_calls_without_overflowing() {
        let one = EngineUsage {
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: 0.01,
        };
        let two = EngineUsage {
            input_tokens: 20,
            output_tokens: 7,
            cost_usd: 0.02,
        };
        let total = one.plus(two);
        assert_eq!(total.input_tokens, 30);
        assert_eq!(total.output_tokens, 12);
        assert!((total.cost_usd - 0.03).abs() < f64::EPSILON * 8.0);
        assert_eq!(total.total_tokens(), 42);

        let huge = EngineUsage {
            input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            cost_usd: 0.0,
        };
        assert_eq!(huge.plus(one).input_tokens, u64::MAX);
        assert_eq!(huge.total_tokens(), u64::MAX);
    }

    #[test]
    fn counts_and_costs_render_compactly() {
        struct Case {
            name: &'static str,
            usage: EngineUsage,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "empty usage has no label",
                usage: EngineUsage::default(),
                want: None,
            },
            Case {
                name: "sub-cent costs keep four decimals",
                usage: EngineUsage {
                    input_tokens: 18_234,
                    output_tokens: 590,
                    cost_usd: 0.0142,
                },
                want: Some("$0.0142 \u{b7} 18.8k tok"),
            },
            Case {
                name: "larger costs round to cents",
                usage: EngineUsage {
                    input_tokens: 1_200_000,
                    output_tokens: 300_000,
                    cost_usd: 1.239,
                },
                want: Some("$1.24 \u{b7} 1.5M tok"),
            },
            Case {
                name: "small counts stay exact",
                usage: EngineUsage {
                    input_tokens: 400,
                    output_tokens: 99,
                    cost_usd: 0.0,
                },
                want: Some("499 tok"),
            },
        ];
        for case in cases {
            assert_eq!(
                usage_label(case.usage).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }
}
