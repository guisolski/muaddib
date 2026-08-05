use crate::core::answer::{Answer, Block, ListItem};
use crate::core::citations::{LinkStatus, block_source_id_slots};
use crate::core::cost::EngineUsage;
use crate::core::mode::Mode;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct EvalCase {
    pub query: String,
    pub mode: Mode,
    pub expect_domains: Vec<String>,
    pub expect_mentions: Vec<String>,
}

impl Default for EvalCase {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: Mode::General,
            expect_domains: Vec::new(),
            expect_mentions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EvalSuite {
    pub cases: Vec<EvalCase>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseReport {
    pub query: String,
    pub mode: Mode,
    pub blocks: usize,
    pub sources: usize,
    pub broken_links: usize,
    pub unchecked_links: usize,
    pub uncited_blocks: usize,
    pub domain_coverage: f64,
    pub mention_coverage: f64,
    pub elapsed_ms: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvalSummary {
    pub cases: usize,
    pub failures: usize,
    pub sources: usize,
    pub broken_links: usize,
    pub uncited_blocks: usize,
    pub domain_coverage: f64,
    pub mention_coverage: f64,
    pub elapsed_ms: u64,
    pub cost_usd: f64,
}

pub fn parse_suite(toml_text: &str) -> Result<EvalSuite, toml::de::Error> {
    toml::from_str(toml_text)
}

pub fn score_answer(
    answer: &Answer,
    case: &EvalCase,
    elapsed_ms: u64,
    usage: Option<&EngineUsage>,
) -> CaseReport {
    CaseReport {
        query: case.query.clone(),
        mode: case.mode,
        blocks: answer.blocks.len(),
        sources: answer.sources.len(),
        broken_links: count_status(answer, |status| {
            matches!(
                status,
                Some(LinkStatus::Invalid(_) | LinkStatus::Unreachable)
            )
        }),
        unchecked_links: count_status(answer, |status| status.is_none()),
        uncited_blocks: uncited_blocks(answer),
        domain_coverage: coverage(&case.expect_domains, |domain| {
            answer
                .sources
                .iter()
                .any(|source| source.url.to_lowercase().contains(&domain.to_lowercase()))
        }),
        mention_coverage: coverage(&case.expect_mentions, |mention| {
            answer_text(answer)
                .to_lowercase()
                .contains(&mention.to_lowercase())
        }),
        elapsed_ms,
        cost_usd: usage.map_or(0.0, |usage| usage.cost_usd),
    }
}

fn count_status(answer: &Answer, matches: impl Fn(Option<LinkStatus>) -> bool) -> usize {
    answer
        .sources
        .iter()
        .filter(|source| matches(source.status))
        .count()
}

fn uncited_blocks(answer: &Answer) -> usize {
    answer
        .blocks
        .iter()
        .filter(|block| carries_citations(block))
        .filter(|block| {
            block_source_id_slots(std::slice::from_ref(*block))
                .iter()
                .all(|slot| slot.is_empty())
        })
        .count()
}

fn carries_citations(block: &Block) -> bool {
    !matches!(block, Block::Heading { .. } | Block::Unknown)
}

fn coverage(expected: &[String], present: impl Fn(&str) -> bool) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let hits = expected.iter().filter(|item| present(item)).count();
    hits as f64 / expected.len() as f64
}

pub fn answer_text(answer: &Answer) -> String {
    let blocks: String = answer.blocks.iter().map(block_text).collect();
    format!("{} {blocks}", answer.title)
}

fn block_text(block: &Block) -> String {
    match block {
        Block::Heading { text, .. } | Block::Paragraph { text, .. } | Block::Quote { text, .. } => {
            format!(" {text}")
        }
        Block::List { items, .. } => items.iter().map(item_text).collect(),
        Block::Table { headers, rows, .. } => {
            format!(" {} {}", headers.join(" "), rows.concat().join(" "))
        }
        Block::Chart { title, labels, .. } => format!(" {title} {}", labels.join(" ")),
        Block::Diagram { title, items, .. } => {
            let labels: String = items
                .iter()
                .map(|item| format!(" {} {}", item.label, item.detail))
                .collect();
            format!(" {title}{labels}")
        }
        Block::Image { caption, .. } => format!(" {caption}"),
        Block::Unknown => String::new(),
    }
}

fn item_text(item: &ListItem) -> String {
    format!(" {}", item.text)
}

pub fn summarize(reports: &[CaseReport], failures: usize) -> EvalSummary {
    let cases = reports.len();
    if cases == 0 {
        return EvalSummary {
            failures,
            ..EvalSummary::default()
        };
    }
    EvalSummary {
        cases,
        failures,
        sources: reports.iter().map(|report| report.sources).sum(),
        broken_links: reports.iter().map(|report| report.broken_links).sum(),
        uncited_blocks: reports.iter().map(|report| report.uncited_blocks).sum(),
        domain_coverage: mean(reports, |report| report.domain_coverage),
        mention_coverage: mean(reports, |report| report.mention_coverage),
        elapsed_ms: reports.iter().map(|report| report.elapsed_ms).sum::<u64>() / cases as u64,
        cost_usd: reports.iter().map(|report| report.cost_usd).sum(),
    }
}

fn mean(reports: &[CaseReport], value: impl Fn(&CaseReport) -> f64) -> f64 {
    reports.iter().map(value).sum::<f64>() / reports.len() as f64
}

pub fn report_markdown(summary: &EvalSummary, reports: &[CaseReport]) -> String {
    let rows: String = reports.iter().map(case_row).collect();
    format!(
        "# Evaluation baseline\n\n\
         Generated by `make eval`. Every number comes from a real engine run, so\n\
         re-generate it after touching prompts, the answer schema, or the grounding\n\
         stages — a diff here is a quality regression.\n\n\
         ## Summary\n\n\
         | metric | value |\n| --- | --- |\n\
         | cases | {} |\n\
         | failed searches | {} |\n\
         | sources | {} |\n\
         | broken links | {} |\n\
         | uncited blocks | {} |\n\
         | expected-domain coverage | {:.0}% |\n\
         | expected-mention coverage | {:.0}% |\n\
         | mean wall clock | {:.1}s |\n\
         | total cost | ${:.4} |\n\n\
         ## Cases\n\n\
         | query | mode | blocks | sources | broken | uncited | domains | mentions | secs |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n{rows}",
        summary.cases,
        summary.failures,
        summary.sources,
        summary.broken_links,
        summary.uncited_blocks,
        summary.domain_coverage * 100.0,
        summary.mention_coverage * 100.0,
        summary.elapsed_ms as f64 / 1000.0,
        summary.cost_usd,
    )
}

fn case_row(report: &CaseReport) -> String {
    format!(
        "| {} | {} | {} | {} | {} | {} | {:.0}% | {:.0}% | {:.1} |\n",
        report.query,
        report.mode.label().to_lowercase(),
        report.blocks,
        report.sources,
        report.broken_links,
        report.uncited_blocks,
        report.domain_coverage * 100.0,
        report.mention_coverage * 100.0,
        report.elapsed_ms as f64 / 1000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{Emphasis, Source};

    fn source(id: u32, url: &str, status: Option<LinkStatus>) -> Source {
        Source {
            id,
            title: format!("s{id}"),
            url: url.to_string(),
            lang: "en".to_string(),
            status,
        }
    }

    fn paragraph(text: &str, source_ids: Vec<u32>) -> Block {
        Block::Paragraph {
            text: text.to_string(),
            source_ids,
            emphasis: Emphasis::None,
        }
    }

    fn case() -> EvalCase {
        EvalCase {
            query: "rust async runtimes".to_string(),
            mode: Mode::General,
            expect_domains: vec!["tokio.rs".to_string(), "docs.rs".to_string()],
            expect_mentions: vec!["Tokio".to_string(), "smol".to_string()],
        }
    }

    #[test]
    fn scoring_counts_the_defects_that_matter() {
        let answer = Answer {
            title: "Rust async runtimes".to_string(),
            blocks: vec![
                Block::Heading {
                    level: 2,
                    text: "Overview".to_string(),
                    emphasis: Emphasis::None,
                },
                paragraph("Tokio is the default.", vec![1]),
                paragraph("This claim has no source.", vec![]),
            ],
            sources: vec![
                source(1, "https://tokio.rs/tutorial", Some(LinkStatus::Valid)),
                source(2, "https://example.com/a", Some(LinkStatus::Invalid(404))),
                source(3, "https://example.com/b", None),
            ],
            ..Answer::default()
        };
        let report = score_answer(&answer, &case(), 31_000, None);
        assert_eq!(report.blocks, 3);
        assert_eq!(report.sources, 3);
        assert_eq!(report.broken_links, 1);
        assert_eq!(report.unchecked_links, 1);
        assert_eq!(report.uncited_blocks, 1);
        assert!((report.domain_coverage - 0.5).abs() < f64::EPSILON);
        assert!((report.mention_coverage - 0.5).abs() < f64::EPSILON);
        assert_eq!(report.elapsed_ms, 31_000);
    }

    #[test]
    fn headings_never_count_as_uncited() {
        let answer = Answer {
            blocks: vec![Block::Heading {
                level: 2,
                text: "Just a heading".to_string(),
                emphasis: Emphasis::None,
            }],
            ..Answer::default()
        };
        let report = score_answer(&answer, &EvalCase::default(), 0, None);
        assert_eq!(report.uncited_blocks, 0);
    }

    #[test]
    fn coverage_of_an_empty_expectation_is_total() {
        let report = score_answer(&Answer::default(), &EvalCase::default(), 0, None);
        assert!((report.domain_coverage - 1.0).abs() < f64::EPSILON);
        assert!((report.mention_coverage - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn mentions_are_searched_across_every_prose_bearing_block() {
        struct Case {
            name: &'static str,
            block: Block,
            mention: &'static str,
        }
        let cases = [
            Case {
                name: "paragraph",
                block: paragraph("mentions tokio here", vec![1]),
                mention: "tokio",
            },
            Case {
                name: "list item",
                block: Block::List {
                    ordered: false,
                    items: vec![ListItem {
                        text: "smol is small".to_string(),
                        source_ids: vec![1],
                    }],
                    emphasis: Emphasis::None,
                },
                mention: "smol",
            },
            Case {
                name: "table cell",
                block: Block::Table {
                    headers: vec!["runtime".to_string()],
                    rows: vec![vec!["async-std".to_string()]],
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                mention: "async-std",
            },
        ];
        for case in cases {
            let answer = Answer {
                blocks: vec![case.block],
                ..Answer::default()
            };
            let expectation = EvalCase {
                expect_mentions: vec![case.mention.to_string()],
                ..EvalCase::default()
            };
            let report = score_answer(&answer, &expectation, 0, None);
            assert!(
                (report.mention_coverage - 1.0).abs() < f64::EPSILON,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn the_summary_averages_coverage_and_sums_the_rest() {
        let reports = vec![
            CaseReport {
                query: "a".to_string(),
                mode: Mode::General,
                blocks: 3,
                sources: 4,
                broken_links: 1,
                unchecked_links: 0,
                uncited_blocks: 1,
                domain_coverage: 1.0,
                mention_coverage: 0.5,
                elapsed_ms: 10_000,
                cost_usd: 0.01,
            },
            CaseReport {
                query: "b".to_string(),
                mode: Mode::Deep,
                blocks: 5,
                sources: 6,
                broken_links: 0,
                unchecked_links: 0,
                uncited_blocks: 0,
                domain_coverage: 0.0,
                mention_coverage: 0.5,
                elapsed_ms: 20_000,
                cost_usd: 0.02,
            },
        ];
        let summary = summarize(&reports, 1);
        assert_eq!(summary.cases, 2);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.sources, 10);
        assert_eq!(summary.broken_links, 1);
        assert_eq!(summary.uncited_blocks, 1);
        assert!((summary.domain_coverage - 0.5).abs() < f64::EPSILON);
        assert!((summary.mention_coverage - 0.5).abs() < f64::EPSILON);
        assert_eq!(summary.elapsed_ms, 15_000);
        assert!((summary.cost_usd - 0.03).abs() < 1e-9);
    }

    #[test]
    fn an_empty_run_summarizes_without_dividing_by_zero() {
        let summary = summarize(&[], 3);
        assert_eq!(summary.cases, 0);
        assert_eq!(summary.failures, 3);
        assert!(summary.domain_coverage.abs() < f64::EPSILON);
    }

    #[test]
    fn the_suite_parses_from_toml() {
        let suite = parse_suite(
            "[[cases]]\n\
             query = \"rust async runtimes\"\n\
             mode = \"deep\"\n\
             expect_domains = [\"tokio.rs\"]\n\
             expect_mentions = [\"Tokio\"]\n\
             \n\
             [[cases]]\n\
             query = \"capital of peru\"\n",
        )
        .unwrap();
        assert_eq!(suite.cases.len(), 2);
        assert_eq!(suite.cases[0].mode, Mode::Deep);
        assert_eq!(suite.cases[0].expect_domains, vec!["tokio.rs".to_string()]);
        assert_eq!(suite.cases[1].mode, Mode::General);
        assert!(suite.cases[1].expect_domains.is_empty());
    }

    #[test]
    fn the_markdown_report_carries_every_case() {
        let reports = vec![CaseReport {
            query: "rust async runtimes".to_string(),
            mode: Mode::Deep,
            blocks: 4,
            sources: 5,
            broken_links: 0,
            unchecked_links: 0,
            uncited_blocks: 0,
            domain_coverage: 1.0,
            mention_coverage: 1.0,
            elapsed_ms: 31_400,
            cost_usd: 0.0142,
        }];
        let rendered = report_markdown(&summarize(&reports, 0), &reports);
        assert!(rendered.contains("# Evaluation baseline"));
        assert!(
            rendered
                .contains("| rust async runtimes | deep | 4 | 5 | 0 | 0 | 100% | 100% | 31.4 |")
        );
        assert!(rendered.contains("| total cost | $0.0142 |"));
    }
}
