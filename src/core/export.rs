use crate::core::answer::{
    Answer, Block, ConflictPosition, DiagramItem, DiagramType, ListItem, Source,
};
use crate::core::citations::LinkStatus;
use crate::core::mode::Mode;

pub const FILENAME_MAX_STEM: usize = 48;
pub const OSC52_MAX_BYTES: usize = 74_000;

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn osc52_payload(text: &str) -> Option<String> {
    let encoded = base64_encode(text.as_bytes());
    (encoded.len() <= OSC52_MAX_BYTES).then(|| format!("\u{1b}]52;c;{encoded}\u{7}"))
}

fn base64_encode(bytes: &[u8]) -> String {
    bytes
        .chunks(3)
        .flat_map(|chunk| {
            let packed = chunk.iter().enumerate().fold(0_u32, |acc, (index, byte)| {
                acc | (u32::from(*byte) << (16 - index * 8))
            });
            (0..4).map(move |slot| match (chunk.len(), slot) {
                (1, 2 | 3) | (2, 3) => '=',
                _ => char::from(BASE64_ALPHABET[((packed >> (18 - slot * 6)) & 0x3f) as usize]),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportContext {
    pub query: String,
    pub mode: Mode,
}

pub fn to_markdown(answer: &Answer, context: &ExportContext) -> String {
    let title = heading_line(1, &answer.title);
    let blocks: String = answer.blocks.iter().map(block_markdown).collect();
    format!(
        "{title}\n{blocks}{}{}",
        sources_markdown(&answer.sources),
        footer_markdown(answer, context)
    )
}

pub fn suggested_filename(answer: &Answer) -> String {
    let stem = slug(&answer.title);
    if stem.is_empty() {
        "muaddib-answer.md".to_string()
    } else {
        format!("muaddib-{stem}.md")
    }
}

fn slug(title: &str) -> String {
    let lowered: String = title
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    lowered
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(FILENAME_MAX_STEM)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn block_markdown(block: &Block) -> String {
    match block {
        Block::Heading { level, text, .. } => format!("{}\n", heading_line(*level + 1, text)),
        Block::Paragraph {
            text, source_ids, ..
        } => format!("{}{}\n\n", text, citation_marks(source_ids)),
        Block::List { ordered, items, .. } => list_markdown(*ordered, items),
        Block::Quote {
            text, source_ids, ..
        } => format!("> {}{}\n\n", text, citation_marks(source_ids)),
        Block::Table {
            headers,
            rows,
            source_ids,
            ..
        } => table_markdown(headers, rows, source_ids),
        Block::Chart {
            title,
            labels,
            values,
            unit,
            source_ids,
            ..
        } => chart_markdown(title, labels, values, unit, source_ids),
        Block::Diagram {
            diagram_type,
            title,
            items,
            source_ids,
            ..
        } => diagram_markdown(*diagram_type, title, items, source_ids),
        Block::Conflict {
            topic, positions, ..
        } => conflict_markdown(topic, positions),
        Block::Image {
            url,
            caption,
            source_ids,
            ..
        } => format!("![{caption}]({url}){}\n\n", citation_marks(source_ids)),
        Block::Unknown => String::new(),
    }
}

fn conflict_markdown(topic: &str, positions: &[ConflictPosition]) -> String {
    let rows: String = positions
        .iter()
        .map(|position| {
            format!(
                "> - {}{}\n",
                position.claim,
                citation_marks(&position.source_ids)
            )
        })
        .collect();
    format!("> [!WARNING]\n> **Sources disagree \u{2014} {topic}**\n>\n{rows}\n")
}

fn heading_line(level: u8, text: &str) -> String {
    let hashes = "#".repeat(usize::from(level.clamp(1, 6)));
    format!("{hashes} {text}\n")
}

fn citation_marks(source_ids: &[u32]) -> String {
    source_ids.iter().map(|id| format!("[{id}]")).collect()
}

fn list_markdown(ordered: bool, items: &[ListItem]) -> String {
    let rows: String = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let bullet = if ordered {
                format!("{}.", index + 1)
            } else {
                "-".to_string()
            };
            format!(
                "{bullet} {}{}\n",
                item.text,
                citation_marks(&item.source_ids)
            )
        })
        .collect();
    format!("{rows}\n")
}

fn table_markdown(headers: &[String], rows: &[Vec<String>], source_ids: &[u32]) -> String {
    if headers.is_empty() {
        return String::new();
    }
    let header_row = format!("| {} |\n", headers.join(" | "));
    let divider = format!("|{}\n", " --- |".repeat(headers.len()));
    let body: String = rows
        .iter()
        .map(|row| format!("| {} |\n", row.join(" | ")))
        .collect();
    format!(
        "{header_row}{divider}{body}{}\n",
        caption_line(source_ids, "")
    )
}

fn chart_markdown(
    title: &str,
    labels: &[String],
    values: &[f64],
    unit: &str,
    source_ids: &[u32],
) -> String {
    let unit_header = if unit.is_empty() { "value" } else { unit };
    let body: String = labels
        .iter()
        .zip(values)
        .map(|(label, value)| format!("| {label} | {} |\n", trim_number(*value)))
        .collect();
    format!(
        "**{title}**\n\n| | {unit_header} |\n| --- | --- |\n{body}{}\n",
        caption_line(source_ids, "")
    )
}

fn trim_number(value: f64) -> String {
    let rendered = format!("{value}");
    rendered
        .strip_suffix(".0")
        .map_or(rendered.clone(), ToString::to_string)
}

fn diagram_markdown(
    diagram_type: DiagramType,
    title: &str,
    items: &[DiagramItem],
    source_ids: &[u32],
) -> String {
    let body = match diagram_type {
        DiagramType::Flow => mermaid_flow(items),
        DiagramType::Timeline => mermaid_timeline(items),
    };
    format!(
        "**{title}**\n\n```mermaid\n{body}```\n{}\n",
        caption_line(source_ids, "")
    )
}

fn mermaid_flow(items: &[DiagramItem]) -> String {
    let nodes: String = items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("    n{index}[\"{}\"]\n", mermaid_text(&item.label)))
        .collect();
    let edges: String = (1..items.len())
        .map(|index| format!("    n{} --> n{index}\n", index - 1))
        .collect();
    format!("flowchart LR\n{nodes}{edges}")
}

fn mermaid_timeline(items: &[DiagramItem]) -> String {
    let rows: String = items
        .iter()
        .map(|item| {
            format!(
                "    {} : {}\n",
                mermaid_text(&item.label),
                mermaid_text(&item.detail)
            )
        })
        .collect();
    format!("timeline\n{rows}")
}

fn mermaid_text(text: &str) -> String {
    text.replace('"', "'").replace([':', '\n'], " ")
}

fn caption_line(source_ids: &[u32], prefix: &str) -> String {
    if source_ids.is_empty() {
        String::new()
    } else {
        format!("{prefix}{}\n", citation_marks(source_ids))
    }
}

fn sources_markdown(sources: &[Source]) -> String {
    if sources.is_empty() {
        return String::new();
    }
    let rows: String = sources
        .iter()
        .map(|source| {
            format!(
                "{}. {}{}[{}]({}){}{}\n",
                source.id,
                status_mark(source.status),
                badge_mark(source),
                escape_brackets(&source.title),
                source.url,
                lang_suffix(&source.lang),
                note_suffix(&source.note)
            )
        })
        .collect();
    format!("## Sources\n\n{rows}\n")
}

fn status_mark(status: Option<LinkStatus>) -> String {
    match status {
        Some(LinkStatus::Invalid(code)) => format!("~~{code}~~ "),
        Some(LinkStatus::Unreachable) => "~~unreachable~~ ".to_string(),
        Some(LinkStatus::Valid) | None => String::new(),
    }
}

fn badge_mark(source: &Source) -> String {
    match source.published {
        Some(year) => format!("`{} {year}` ", source.class.label()),
        None => format!("`{}` ", source.class.label()),
    }
}

fn note_suffix(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!(" \u{2014} {note}")
    }
}

fn escape_brackets(title: &str) -> String {
    title.replace('[', "\\[").replace(']', "\\]")
}

fn lang_suffix(lang: &str) -> String {
    if lang.is_empty() {
        String::new()
    } else {
        format!(" ({lang})")
    }
}

fn footer_markdown(answer: &Answer, context: &ExportContext) -> String {
    let followups = followups_markdown(&answer.followups);
    format!(
        "{followups}---\n\nSearched with [muaddib](https://github.com/guisolski/muaddib) \
         \u{2014} `{}` mode \u{2014} query: {}\n",
        context.mode.label().to_lowercase(),
        context.query
    )
}

fn followups_markdown(followups: &[String]) -> String {
    if followups.is_empty() {
        return String::new();
    }
    let rows: String = followups
        .iter()
        .map(|followup| format!("- {followup}\n"))
        .collect();
    format!("## Follow-ups\n\n{rows}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::Emphasis;

    fn context() -> ExportContext {
        ExportContext {
            query: "rust async runtimes".to_string(),
            mode: Mode::General,
        }
    }

    fn source(id: u32, status: Option<LinkStatus>) -> Source {
        Source {
            id,
            title: format!("Source {id}"),
            url: format!("https://example.com/{id}"),
            lang: "en".to_string(),
            status,
            ..Default::default()
        }
    }

    struct BlockCase {
        name: &'static str,
        block: Block,
        want_contains: &'static str,
    }

    fn block_cases() -> Vec<BlockCase> {
        let mut cases = text_block_cases();
        cases.extend(visual_block_cases());
        cases
    }

    fn text_block_cases() -> Vec<BlockCase> {
        type Case = BlockCase;
        vec![
            Case {
                name: "heading demotes below the document title",
                block: Block::Heading {
                    level: 2,
                    text: "Runtimes".to_string(),
                    emphasis: Emphasis::None,
                },
                want_contains: "### Runtimes",
            },
            Case {
                name: "paragraph keeps its citations",
                block: Block::Paragraph {
                    text: "Tokio is the default.".to_string(),
                    source_ids: vec![1, 2],
                    emphasis: Emphasis::None,
                },
                want_contains: "Tokio is the default.[1][2]",
            },
            Case {
                name: "unordered list uses dashes",
                block: Block::List {
                    ordered: false,
                    items: vec![ListItem {
                        text: "smol is small".to_string(),
                        source_ids: vec![3],
                    }],
                    emphasis: Emphasis::None,
                },
                want_contains: "- smol is small[3]",
            },
            Case {
                name: "ordered list numbers its items",
                block: Block::List {
                    ordered: true,
                    items: vec![ListItem {
                        text: "first".to_string(),
                        source_ids: vec![],
                    }],
                    emphasis: Emphasis::None,
                },
                want_contains: "1. first",
            },
            Case {
                name: "quote uses a blockquote marker",
                block: Block::Quote {
                    text: "async is hard".to_string(),
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                want_contains: "> async is hard[1]",
            },
        ]
    }

    fn visual_block_cases() -> Vec<BlockCase> {
        type Case = BlockCase;
        vec![
            Case {
                name: "table becomes a gfm pipe table",
                block: Block::Table {
                    headers: vec!["runtime".to_string(), "stars".to_string()],
                    rows: vec![vec!["tokio".to_string(), "25k".to_string()]],
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
                want_contains: "| runtime | stars |\n| --- | --- |\n| tokio | 25k |",
            },
            Case {
                name: "chart becomes a label value table",
                block: Block::Chart {
                    chart_type: crate::core::answer::ChartType::Bar,
                    title: "Adoption".to_string(),
                    labels: vec!["tokio".to_string()],
                    values: vec![70.0],
                    unit: "percent".to_string(),
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
                want_contains: "| tokio | 70 |",
            },
            Case {
                name: "flow diagram becomes a mermaid flowchart",
                block: Block::Diagram {
                    diagram_type: DiagramType::Flow,
                    title: "Pipeline".to_string(),
                    items: vec![
                        DiagramItem {
                            label: "expand".to_string(),
                            detail: String::new(),
                        },
                        DiagramItem {
                            label: "search".to_string(),
                            detail: String::new(),
                        },
                    ],
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
                want_contains: "```mermaid\nflowchart LR\n    n0[\"expand\"]\n    n1[\"search\"]\n    n0 --> n1\n```",
            },
            Case {
                name: "timeline diagram becomes a mermaid timeline",
                block: Block::Diagram {
                    diagram_type: DiagramType::Timeline,
                    title: "History".to_string(),
                    items: vec![DiagramItem {
                        label: "2019".to_string(),
                        detail: "tokio 0.2".to_string(),
                    }],
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
                want_contains: "```mermaid\ntimeline\n    2019 : tokio 0.2\n```",
            },
            Case {
                name: "image becomes a markdown image",
                block: Block::Image {
                    url: "https://example.com/a.png".to_string(),
                    caption: "a chart".to_string(),
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
                want_contains: "![a chart](https://example.com/a.png)",
            },
            Case {
                name: "conflict becomes a github warning callout",
                block: Block::Conflict {
                    topic: "projected 2027 capacity".to_string(),
                    kind: crate::core::answer::ConflictKind::Direct,
                    positions: vec![
                        ConflictPosition {
                            claim: "IEA reports 4.1 TW".to_string(),
                            source_ids: vec![2],
                        },
                        ConflictPosition {
                            claim: "IRENA reports 3.4 TW".to_string(),
                            source_ids: vec![5],
                        },
                    ],
                    emphasis: Emphasis::None,
                },
                want_contains: "> [!WARNING]\n> **Sources disagree \u{2014} projected 2027 capacity**\n>\n> - IEA reports 4.1 TW[2]\n> - IRENA reports 3.4 TW[5]",
            },
            Case {
                name: "unknown blocks vanish",
                block: Block::Unknown,
                want_contains: "",
            },
        ]
    }

    #[test]
    fn every_block_variant_renders_to_markdown() {
        for case in block_cases() {
            let answer = Answer {
                title: "T".to_string(),
                blocks: vec![case.block],
                ..Answer::default()
            };
            let rendered = to_markdown(&answer, &context());
            assert!(
                rendered.contains(case.want_contains),
                "{}\n--- got ---\n{rendered}",
                case.name
            );
        }
    }

    #[test]
    fn the_document_opens_with_a_single_h1_title() {
        let answer = Answer {
            title: "Rust async runtimes".to_string(),
            ..Answer::default()
        };
        let rendered = to_markdown(&answer, &context());
        assert!(rendered.starts_with("# Rust async runtimes\n"));
        assert_eq!(rendered.matches("\n# ").count(), 0);
    }

    #[test]
    fn sources_carry_their_link_status() {
        struct Case {
            name: &'static str,
            status: Option<LinkStatus>,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "valid links render plainly",
                status: Some(LinkStatus::Valid),
                want: "1. `unclassified` [Source 1](https://example.com/1) (en)",
            },
            Case {
                name: "unchecked links render plainly",
                status: None,
                want: "1. `unclassified` [Source 1](https://example.com/1) (en)",
            },
            Case {
                name: "broken links are struck through with the code",
                status: Some(LinkStatus::Invalid(404)),
                want: "1. ~~404~~ `unclassified` [Source 1](https://example.com/1) (en)",
            },
            Case {
                name: "unreachable links say so",
                status: Some(LinkStatus::Unreachable),
                want: "1. ~~unreachable~~ `unclassified` [Source 1](https://example.com/1) (en)",
            },
        ];
        for case in cases {
            let answer = Answer {
                sources: vec![source(1, case.status)],
                ..Answer::default()
            };
            let rendered = to_markdown(&answer, &context());
            assert!(
                rendered.contains(case.want),
                "{}\n--- got ---\n{rendered}",
                case.name
            );
        }
    }

    #[test]
    fn the_footer_records_the_query_and_mode() {
        let answer = Answer::default();
        let rendered = to_markdown(
            &answer,
            &ExportContext {
                query: "CRISPR delivery".to_string(),
                mode: Mode::Scientific,
            },
        );
        assert!(rendered.contains("`scientific` mode"));
        assert!(rendered.contains("query: CRISPR delivery"));
    }

    #[test]
    fn empty_sections_are_omitted_entirely() {
        let rendered = to_markdown(&Answer::default(), &context());
        assert!(!rendered.contains("## Sources"));
        assert!(!rendered.contains("## Follow-ups"));
    }

    #[test]
    fn mermaid_labels_lose_characters_that_would_break_the_diagram() {
        let answer = Answer {
            blocks: vec![Block::Diagram {
                diagram_type: DiagramType::Timeline,
                title: "T".to_string(),
                items: vec![DiagramItem {
                    label: "a \"quoted\" step".to_string(),
                    detail: "with: a colon".to_string(),
                }],
                source_ids: vec![],
                emphasis: Emphasis::None,
            }],
            ..Answer::default()
        };
        let rendered = to_markdown(&answer, &context());
        assert!(rendered.contains("a 'quoted' step : with  a colon"));
    }

    #[test]
    fn filenames_are_slugged_and_bounded() {
        struct Case {
            name: &'static str,
            title: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "plain title",
                title: "Rust async runtimes",
                want: "muaddib-rust-async-runtimes.md",
            },
            Case {
                name: "punctuation collapses",
                title: "What is CRISPR?! (2026)",
                want: "muaddib-what-is-crispr-2026.md",
            },
            Case {
                name: "empty title falls back",
                title: "",
                want: "muaddib-answer.md",
            },
            Case {
                name: "punctuation-only title falls back",
                title: "???",
                want: "muaddib-answer.md",
            },
        ];
        for case in cases {
            let answer = Answer {
                title: case.title.to_string(),
                ..Answer::default()
            };
            assert_eq!(suggested_filename(&answer), case.want, "{}", case.name);
        }
    }

    #[test]
    fn base64_matches_the_rfc_4648_test_vectors() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "empty",
                input: "",
                want: "",
            },
            Case {
                name: "one byte pads twice",
                input: "f",
                want: "Zg==",
            },
            Case {
                name: "two bytes pad once",
                input: "fo",
                want: "Zm8=",
            },
            Case {
                name: "three bytes need no padding",
                input: "foo",
                want: "Zm9v",
            },
            Case {
                name: "four bytes",
                input: "foob",
                want: "Zm9vYg==",
            },
            Case {
                name: "five bytes",
                input: "fooba",
                want: "Zm9vYmE=",
            },
            Case {
                name: "six bytes",
                input: "foobar",
                want: "Zm9vYmFy",
            },
            Case {
                name: "multibyte utf-8 survives",
                input: "olá",
                want: "b2zDoQ==",
            },
        ];
        for case in cases {
            assert_eq!(
                base64_encode(case.input.as_bytes()),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn osc52_wraps_the_payload_and_refuses_oversized_text() {
        let payload = osc52_payload("foobar").expect("small text fits");
        assert_eq!(payload, "\u{1b}]52;c;Zm9vYmFy\u{7}");

        let huge = "a".repeat(OSC52_MAX_BYTES);
        assert!(osc52_payload(&huge).is_none());
    }

    #[test]
    fn long_titles_stay_within_the_filename_budget() {
        let answer = Answer {
            title: "a ".repeat(200),
            ..Answer::default()
        };
        let name = suggested_filename(&answer);
        assert!(
            name.len() <= FILENAME_MAX_STEM + "muaddib-.md".len(),
            "{name}"
        );
    }
}
