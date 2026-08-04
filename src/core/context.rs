use crate::core::answer::{Answer, Block};
use crate::core::citations::{is_valid_source_url, normalize_url};
use crate::core::readability::excerpt;
use crate::core::tree::{NodeId, ResearchNode, ResearchTree};
use std::collections::BTreeSet;

pub const MAX_CONTEXT_STEPS: usize = 4;
pub const DIGEST_MAX_CHARS: usize = 500;
pub const MAX_STEP_SOURCES: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResearchContext {
    pub steps: Vec<ContextStep>,
    pub omitted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextStep {
    pub query: String,
    pub summary: String,
    pub source_urls: Vec<String>,
}

pub fn context_for(tree: &ResearchTree, node: NodeId) -> ResearchContext {
    let path = tree.ancestors(node);
    let omitted = path.len().saturating_sub(MAX_CONTEXT_STEPS);
    let steps = path
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx == 0 || *idx >= omitted + usize::from(omitted > 0))
        .map(|(_, ancestor)| step_for(ancestor))
        .collect();
    ResearchContext { steps, omitted }
}

fn step_for(node: &ResearchNode) -> ContextStep {
    ContextStep {
        query: node.query.clone(),
        summary: answer_digest(&node.answer, DIGEST_MAX_CHARS),
        source_urls: node
            .answer
            .sources
            .iter()
            .take(MAX_STEP_SOURCES)
            .map(|source| source.url.clone())
            .collect(),
    }
}

pub fn answer_digest(answer: &Answer, max_chars: usize) -> String {
    let prose = answer.blocks.iter().find_map(first_prose);
    let digest = match (answer.title.trim(), prose) {
        ("", None) => String::new(),
        ("", Some(body)) => body,
        (title, None) => title.to_string(),
        (title, Some(body)) => format!("{title} \u{2014} {body}"),
    };
    excerpt(&digest, max_chars)
}

fn first_prose(block: &Block) -> Option<String> {
    match block {
        Block::Paragraph { text, .. } => Some(text.clone()),
        Block::List { items, .. } => Some(
            items
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>()
                .join("; "),
        ),
        _ => None,
    }
}

pub fn context_prompt_block(context: &ResearchContext) -> String {
    if context.steps.is_empty() {
        return String::new();
    }
    let steps: String = context
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| step_lines(idx, step, context))
        .collect();
    format!(
        "This search continues a research thread. Earlier steps, oldest first:\n\
         {steps}\
         Answer the new query in that context: build on the earlier answers instead of \
         repeating them, and prioritize new ground. You may cite the listed source URLs \
         again when they support a claim.\n"
    )
}

fn step_lines(idx: usize, step: &ContextStep, context: &ResearchContext) -> String {
    let elision = if idx == 1 && context.omitted > 0 {
        format!("(\u{2026} {} earlier steps omitted)\n", context.omitted)
    } else {
        String::new()
    };
    let sources = if step.source_urls.is_empty() {
        String::new()
    } else {
        format!("   sources: {}\n", step.source_urls.join(" "))
    };
    format!(
        "{elision}{}. {}\n   answer: {}\n{sources}",
        idx + 1,
        step.query,
        step.summary,
    )
}

pub fn context_allowed_urls(context: &ResearchContext) -> BTreeSet<String> {
    context
        .steps
        .iter()
        .flat_map(|step| step.source_urls.iter())
        .filter(|url| is_valid_source_url(url))
        .map(|url| normalize_url(url))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{ListItem, Source};
    use crate::core::mode::Mode;
    use crate::core::tree::NodeSeed;

    fn answer(title: &str, paragraph: &str, urls: &[&str]) -> Answer {
        Answer {
            title: title.to_string(),
            blocks: vec![Block::Paragraph {
                text: paragraph.to_string(),
                source_ids: vec![1],
                emphasis: crate::core::answer::Emphasis::None,
            }],
            sources: urls
                .iter()
                .enumerate()
                .map(|(idx, url)| Source {
                    id: idx as u32 + 1,
                    title: format!("source {idx}"),
                    url: (*url).to_string(),
                    lang: "en".to_string(),
                })
                .collect(),
            ..Answer::default()
        }
    }

    fn seed(query: &str, urls: &[&str]) -> NodeSeed {
        NodeSeed {
            query: query.to_string(),
            mode: Mode::Scientific,
            fast: false,
            started_at: 0,
            completed_at: 0,
            answer: answer(
                &format!("Title of {query}"),
                &format!("Body of {query}."),
                urls,
            ),
            sub_queries: Vec::new(),
            web_urls: Vec::new(),
        }
    }

    fn chain(tree: &mut ResearchTree, queries: &[&str]) -> NodeId {
        queries.iter().fold(None, |parent, query| {
            Some(tree.add_node(parent, seed(query, &["https://one.example/a"])))
        });
        tree.current.unwrap()
    }

    #[test]
    fn context_collects_the_ancestor_path_in_order() {
        let mut tree = ResearchTree::default();
        let leaf = chain(&mut tree, &["root", "mid", "leaf"]);
        let context = context_for(&tree, leaf);
        let queries: Vec<&str> = context
            .steps
            .iter()
            .map(|step| step.query.as_str())
            .collect();
        assert_eq!(queries, vec!["root", "mid", "leaf"]);
        assert_eq!(context.omitted, 0);
        assert!(context.steps[0].summary.contains("Title of root"));
        assert!(context.steps[0].summary.contains("Body of root."));
    }

    #[test]
    fn long_chains_keep_the_root_and_the_latest_steps() {
        let mut tree = ResearchTree::default();
        let leaf = chain(&mut tree, &["a", "b", "c", "d", "e", "f"]);
        let context = context_for(&tree, leaf);
        let queries: Vec<&str> = context
            .steps
            .iter()
            .map(|step| step.query.as_str())
            .collect();
        assert_eq!(queries, vec!["a", "d", "e", "f"]);
        assert_eq!(context.omitted, 2);
        let block = context_prompt_block(&context);
        assert!(block.contains("2 earlier steps omitted"));
    }

    #[test]
    fn unknown_node_yields_an_empty_context_and_block() {
        let context = context_for(&ResearchTree::default(), 7);
        assert_eq!(context, ResearchContext::default());
        assert_eq!(context_prompt_block(&context), "");
    }

    #[test]
    fn prompt_block_lists_steps_summaries_and_sources() {
        let mut tree = ResearchTree::default();
        let leaf = chain(&mut tree, &["root", "leaf"]);
        let block = context_prompt_block(&context_for(&tree, leaf));
        assert!(block.contains("1. root"));
        assert!(block.contains("2. leaf"));
        assert!(block.contains("answer: Title of root \u{2014} Body of root."));
        assert!(block.contains("sources: https://one.example/a"));
        assert!(block.contains("build on the earlier answers"));
    }

    #[test]
    fn answer_digest_prefers_title_and_first_prose_and_truncates() {
        struct Case {
            name: &'static str,
            answer: Answer,
            max_chars: usize,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "title plus paragraph",
                answer: answer("Title", "Body.", &[]),
                max_chars: 100,
                want: "Title \u{2014} Body.",
            },
            Case {
                name: "list items join when no paragraph exists",
                answer: Answer {
                    title: "T".to_string(),
                    blocks: vec![Block::List {
                        ordered: false,
                        items: vec![
                            ListItem {
                                text: "one".to_string(),
                                source_ids: vec![],
                            },
                            ListItem {
                                text: "two".to_string(),
                                source_ids: vec![],
                            },
                        ],
                        emphasis: crate::core::answer::Emphasis::None,
                    }],
                    ..Answer::default()
                },
                max_chars: 100,
                want: "T \u{2014} one; two",
            },
            Case {
                name: "long digests truncate with an ellipsis",
                answer: answer("Title", "A very long body indeed.", &[]),
                max_chars: 8,
                want: "Title \u{2014} \u{2026}",
            },
            Case {
                name: "empty answer digests to nothing",
                answer: Answer::default(),
                max_chars: 10,
                want: "",
            },
        ];
        for case in cases {
            assert_eq!(
                answer_digest(&case.answer, case.max_chars),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn allowed_urls_normalize_and_drop_invalid_sources() {
        let context = ResearchContext {
            steps: vec![ContextStep {
                query: "q".to_string(),
                summary: String::new(),
                source_urls: vec![
                    "https://One.example/Path/".to_string(),
                    "not-a-url".to_string(),
                ],
            }],
            omitted: 0,
        };
        let allowed = context_allowed_urls(&context);
        assert!(allowed.contains("https://one.example/Path"));
        assert_eq!(allowed.len(), 1);
    }
}
