use crate::core::readability::excerpt;
use crate::core::tree::{ResearchNode, ResearchTree};

pub fn tree_lines(tree: &ResearchTree, width: usize) -> Vec<String> {
    tree.flatten()
        .iter()
        .map(|row| {
            let label = tree.node(row.id).map_or_else(String::new, node_label);
            let line = format!("{}{label}", branch_prefix(&row.last_by_level));
            excerpt(&line, width.max(8))
        })
        .collect()
}

fn branch_prefix(last_by_level: &[bool]) -> String {
    let depth = last_by_level.len().saturating_sub(1);
    if depth == 0 {
        return "\u{25cf} ".to_string();
    }
    let pipes: String = last_by_level[1..depth]
        .iter()
        .map(|last| if *last { "   " } else { "\u{2502}  " })
        .collect();
    let connector = if last_by_level[depth] {
        "\u{2514}\u{2500} "
    } else {
        "\u{251c}\u{2500} "
    };
    format!("  {pipes}{connector}")
}

fn node_label(node: &ResearchNode) -> String {
    let fast = if node.fast { " \u{26a1}" } else { "" };
    format!(
        "{} \u{00b7} {}{fast} \u{00b7} {} sources",
        node.query,
        node.mode.label(),
        node.answer.sources.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::Answer;
    use crate::core::mode::Mode;
    use crate::core::tree::NodeSeed;

    fn seed(query: &str) -> NodeSeed {
        NodeSeed {
            query: query.to_string(),
            mode: Mode::General,
            fast: false,
            started_at: 0,
            completed_at: 0,
            answer: Answer::default(),
            sub_queries: Vec::new(),
            web_urls: Vec::new(),
        }
    }

    #[test]
    fn tree_lines_draw_branch_prefixes_for_common_shapes() {
        struct Case {
            name: &'static str,
            build: fn(&mut ResearchTree),
            want_starts: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "single root",
                build: |tree| {
                    tree.add_node(None, seed("alpha"));
                },
                want_starts: vec!["\u{25cf} alpha"],
            },
            Case {
                name: "root with two children",
                build: |tree| {
                    let root = tree.add_node(None, seed("alpha"));
                    tree.add_node(Some(root), seed("beta"));
                    tree.add_node(Some(root), seed("gamma"));
                },
                want_starts: vec![
                    "\u{25cf} alpha",
                    "  \u{251c}\u{2500} beta",
                    "  \u{2514}\u{2500} gamma",
                ],
            },
            Case {
                name: "nested branch keeps the ancestor pipe",
                build: |tree| {
                    let root = tree.add_node(None, seed("alpha"));
                    let mid = tree.add_node(Some(root), seed("beta"));
                    tree.add_node(Some(root), seed("gamma"));
                    tree.add_node(Some(mid), seed("delta"));
                },
                want_starts: vec![
                    "\u{25cf} alpha",
                    "  \u{251c}\u{2500} beta",
                    "  \u{2502}  \u{2514}\u{2500} delta",
                    "  \u{2514}\u{2500} gamma",
                ],
            },
            Case {
                name: "forest of two roots",
                build: |tree| {
                    tree.add_node(None, seed("alpha"));
                    tree.add_node(None, seed("beta"));
                },
                want_starts: vec!["\u{25cf} alpha", "\u{25cf} beta"],
            },
        ];
        for case in cases {
            let mut tree = ResearchTree::default();
            (case.build)(&mut tree);
            let lines = tree_lines(&tree, 60);
            assert_eq!(lines.len(), case.want_starts.len(), "{}", case.name);
            for (line, want) in lines.iter().zip(&case.want_starts) {
                assert!(
                    line.starts_with(want),
                    "{}: {line:?} does not start with {want:?}",
                    case.name
                );
            }
        }
    }

    #[test]
    fn tree_lines_carry_mode_fast_and_source_count() {
        let mut tree = ResearchTree::default();
        tree.add_node(
            None,
            NodeSeed {
                fast: true,
                mode: Mode::Scientific,
                ..seed("alpha")
            },
        );
        let lines = tree_lines(&tree, 80);
        assert!(lines[0].contains("Scientific"));
        assert!(lines[0].contains('\u{26a1}'));
        assert!(lines[0].contains("0 sources"));
    }

    #[test]
    fn long_rows_truncate_to_the_width() {
        let mut tree = ResearchTree::default();
        tree.add_node(None, seed(&"long query ".repeat(20)));
        let lines = tree_lines(&tree, 30);
        assert!(lines[0].chars().count() <= 31);
        assert!(lines[0].ends_with('\u{2026}'));
    }
}
