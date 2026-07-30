const BAR_GLYPH: &str = "▇";
const MIN_BAR_WIDTH: usize = 4;
const MAX_LABEL_WIDTH: usize = 18;

pub fn bar_chart_lines(
    labels: &[String],
    values: &[f64],
    unit: &str,
    width: u16,
    fraction: f64,
) -> Vec<String> {
    let pairs: Vec<(&String, f64)> = labels
        .iter()
        .zip(values.iter().copied())
        .filter(|(_, value)| value.is_finite())
        .collect();
    if pairs.is_empty() {
        return Vec::new();
    }
    let max_magnitude = pairs
        .iter()
        .map(|(_, value)| value.abs())
        .fold(0.0_f64, f64::max);
    let label_width = pairs
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0)
        .min(MAX_LABEL_WIDTH);
    let bar_width = usable_bar_width(width, label_width);
    pairs
        .iter()
        .map(|(label, value)| {
            chart_row(
                label,
                *value,
                unit,
                label_width,
                bar_width,
                max_magnitude,
                fraction,
            )
        })
        .collect()
}

fn usable_bar_width(total_width: u16, label_width: usize) -> usize {
    let reserved = label_width + 12;
    (usize::from(total_width))
        .saturating_sub(reserved)
        .max(MIN_BAR_WIDTH)
}

fn chart_row(
    label: &str,
    value: f64,
    unit: &str,
    label_width: usize,
    bar_width: usize,
    max_magnitude: f64,
    fraction: f64,
) -> String {
    let clipped: String = label.chars().take(label_width).collect();
    let full_length = bar_length(value, max_magnitude, bar_width) as f64;
    let shown = (full_length * fraction.clamp(0.0, 1.0)).round() as usize;
    let bar = BAR_GLYPH.repeat(shown);
    let value_text = format_value(value, unit);
    format!("{clipped:>label_width$} {bar} {value_text}")
}

fn bar_length(value: f64, max_magnitude: f64, bar_width: usize) -> usize {
    if value <= 0.0 || max_magnitude <= 0.0 {
        return 0;
    }
    let scaled = (value / max_magnitude * bar_width as f64).round() as usize;
    scaled.clamp(1, bar_width)
}

fn format_value(value: f64, unit: &str) -> String {
    let number = if (value.fract()).abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(names: &[&str]) -> Vec<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn bars_grow_with_fraction() {
        struct Case {
            name: &'static str,
            fraction: f64,
            want_glyphs: usize,
        }
        let cases = [
            Case {
                name: "zero fraction draws no bar",
                fraction: 0.0,
                want_glyphs: 0,
            },
            Case {
                name: "half fraction draws half the bar",
                fraction: 0.5,
                want_glyphs: 14,
            },
            Case {
                name: "full fraction matches the settled bar",
                fraction: 1.0,
                want_glyphs: 27,
            },
        ];
        for case in cases {
            let lines = bar_chart_lines(&labels(&["a"]), &[10.0], "", 40, case.fraction);
            assert_eq!(
                lines[0].matches(BAR_GLYPH).count(),
                case.want_glyphs,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn bars_scale_relative_to_the_maximum_value() {
        let lines = bar_chart_lines(&labels(&["a", "b"]), &[100.0, 50.0], "%", 60, 1.0);
        let bars: Vec<usize> = lines
            .iter()
            .map(|line| line.matches(BAR_GLYPH).count())
            .collect();
        assert_eq!(bars.len(), 2);
        assert!(bars[0] > bars[1]);
        let ratio_error = bars[0] as i64 - 2 * bars[1] as i64;
        assert!(ratio_error.abs() <= 1, "ratio error: {ratio_error}");
    }

    #[test]
    fn chart_edge_cases_never_panic() {
        struct Case {
            name: &'static str,
            labels: Vec<String>,
            values: Vec<f64>,
            want_rows: usize,
        }
        let cases = [
            Case {
                name: "empty input",
                labels: vec![],
                values: vec![],
                want_rows: 0,
            },
            Case {
                name: "all zeros",
                labels: labels(&["a", "b"]),
                values: vec![0.0, 0.0],
                want_rows: 2,
            },
            Case {
                name: "negative values get no bar",
                labels: labels(&["down"]),
                values: vec![-5.0],
                want_rows: 1,
            },
            Case {
                name: "mismatched lengths zip to the shorter",
                labels: labels(&["a", "b", "c"]),
                values: vec![1.0],
                want_rows: 1,
            },
            Case {
                name: "non finite values are dropped",
                labels: labels(&["nan", "ok"]),
                values: vec![f64::NAN, 2.0],
                want_rows: 1,
            },
        ];
        for case in cases {
            let lines = bar_chart_lines(&case.labels, &case.values, "", 40, 1.0);
            assert_eq!(lines.len(), case.want_rows, "{}", case.name);
        }
    }

    #[test]
    fn tiny_widths_still_produce_output() {
        let lines = bar_chart_lines(&labels(&["label"]), &[10.0], "%", 1, 1.0);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(BAR_GLYPH));
    }

    #[test]
    fn positive_values_always_show_at_least_one_block() {
        let lines = bar_chart_lines(&labels(&["big", "tiny"]), &[1000.0, 1.0], "", 40, 1.0);
        assert!(lines[1].contains(BAR_GLYPH));
    }

    #[test]
    fn values_format_with_unit_and_precision() {
        assert_eq!(format_value(42.0, "%"), "42 %");
        assert_eq!(format_value(41.5, ""), "41.5");
    }
}
