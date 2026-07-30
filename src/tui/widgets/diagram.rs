use crate::core::answer::{DiagramItem, DiagramType};
use crate::tui::widgets::textwrap::wrap_text;

const NODE_PREFIX: &str = "● ";
const CONTINUATION_INDENT: &str = "  ";
const FLOW_CONNECTOR: &str = "▼";
const TIMELINE_CONNECTOR: &str = "│";

pub fn diagram_lines(
    diagram_type: DiagramType,
    items: &[DiagramItem],
    width: usize,
    fraction: f64,
) -> Vec<String> {
    let shown = visible_items(items.len(), fraction);
    let usable = width.saturating_sub(NODE_PREFIX.chars().count()).max(4);
    let mut out = Vec::new();
    for (index, item) in items.iter().take(shown).enumerate() {
        if index > 0 {
            out.push(connector(diagram_type).to_string());
        }
        append_wrapped(&mut out, &item.label, usable, NODE_PREFIX);
        if !item.detail.trim().is_empty() {
            append_wrapped(&mut out, &item.detail, usable, CONTINUATION_INDENT);
        }
    }
    out
}

fn append_wrapped(out: &mut Vec<String>, text: &str, width: usize, first_prefix: &str) {
    for (index, row) in wrap_text(text, width).into_iter().enumerate() {
        let prefix = if index == 0 {
            first_prefix
        } else {
            CONTINUATION_INDENT
        };
        out.push(format!("{prefix}{row}"));
    }
}

fn visible_items(count: usize, fraction: f64) -> usize {
    let scaled = (count as f64 * fraction.clamp(0.0, 1.0)).ceil() as usize;
    scaled.min(count)
}

fn connector(diagram_type: DiagramType) -> &'static str {
    match diagram_type {
        DiagramType::Flow => FLOW_CONNECTOR,
        DiagramType::Timeline => TIMELINE_CONNECTOR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, detail: &str) -> DiagramItem {
        DiagramItem {
            label: label.to_string(),
            detail: detail.to_string(),
        }
    }

    #[test]
    fn flow_and_timeline_use_their_connectors() {
        struct Case {
            name: &'static str,
            diagram_type: DiagramType,
            want_connector: &'static str,
        }
        let cases = [
            Case {
                name: "flow steps point downward",
                diagram_type: DiagramType::Flow,
                want_connector: "▼",
            },
            Case {
                name: "timeline events share a spine",
                diagram_type: DiagramType::Timeline,
                want_connector: "│",
            },
        ];
        let items = [item("First", ""), item("Second", "")];
        for case in cases {
            let lines = diagram_lines(case.diagram_type, &items, 40, 1.0);
            assert_eq!(
                lines,
                vec!["● First", case.want_connector, "● Second"],
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn details_wrap_indented_under_their_label() {
        let items = [item(
            "Expand",
            "one engine call turns the query into sub-queries",
        )];
        let lines = diagram_lines(DiagramType::Flow, &items, 24, 1.0);
        assert_eq!(lines[0], "● Expand");
        assert!(lines.len() > 2);
        for detail_row in &lines[1..] {
            assert!(detail_row.starts_with("  "), "{detail_row}");
        }
    }

    #[test]
    fn items_reveal_with_fraction() {
        struct Case {
            name: &'static str,
            fraction: f64,
            want_items: usize,
        }
        let cases = [
            Case {
                name: "zero fraction shows nothing",
                fraction: 0.0,
                want_items: 0,
            },
            Case {
                name: "half fraction shows half the items",
                fraction: 0.5,
                want_items: 2,
            },
            Case {
                name: "full fraction shows everything",
                fraction: 1.0,
                want_items: 4,
            },
        ];
        let items = [item("a", ""), item("b", ""), item("c", ""), item("d", "")];
        for case in cases {
            let lines = diagram_lines(DiagramType::Flow, &items, 40, case.fraction);
            let nodes = lines
                .iter()
                .filter(|line| line.starts_with(NODE_PREFIX))
                .count();
            assert_eq!(nodes, case.want_items, "{}", case.name);
        }
    }

    #[test]
    fn edge_cases_never_panic() {
        struct Case {
            name: &'static str,
            items: Vec<DiagramItem>,
            width: usize,
            want_empty: bool,
        }
        let cases = [
            Case {
                name: "no items render nothing",
                items: vec![],
                width: 40,
                want_empty: true,
            },
            Case {
                name: "zero width still renders",
                items: vec![item("label", "detail")],
                width: 0,
                want_empty: false,
            },
            Case {
                name: "tiny width still renders",
                items: vec![item("label", "detail")],
                width: 1,
                want_empty: false,
            },
        ];
        for case in cases {
            let lines = diagram_lines(DiagramType::Timeline, &case.items, case.width, 1.0);
            assert_eq!(lines.is_empty(), case.want_empty, "{}", case.name);
        }
    }
}
