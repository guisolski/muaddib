use crate::core::answer::{Answer, Block, Source};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub claim: String,
    #[serde(default)]
    pub source_title: String,
    pub source_url: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubSearchResponse {
    pub summary: String,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubResult {
    pub query: String,
    pub lang: String,
    pub response: SubSearchResponse,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MergedFindings {
    pub findings: Vec<Finding>,
    pub summaries: Vec<String>,
}

pub fn parse_sub_response(value: &serde_json::Value) -> Option<SubSearchResponse> {
    serde_json::from_value(value.clone()).ok()
}

pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let normalized = match without_fragment.split_once("://") {
        Some((scheme, rest)) => join_scheme_host_path(scheme, rest),
        None => without_fragment.to_string(),
    };
    normalized.trim_end_matches('/').to_string()
}

fn join_scheme_host_path(scheme: &str, rest: &str) -> String {
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, Some(path)),
        None => (rest, None),
    };
    let mut joined = format!(
        "{}://{}",
        scheme.to_ascii_lowercase(),
        host.to_ascii_lowercase()
    );
    if let Some(path) = path {
        joined.push('/');
        joined.push_str(path);
    }
    joined
}

pub fn is_valid_source_url(url: &str) -> bool {
    let trimmed = url.trim();
    let after_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"));
    after_scheme.is_some_and(|rest| !rest.is_empty())
}

pub fn merge_sub_results(results: &[SubResult]) -> MergedFindings {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut merged = MergedFindings::default();
    for result in results {
        if !result.response.summary.trim().is_empty() {
            merged
                .summaries
                .push(format!("[{}] {}", result.query, result.response.summary));
        }
        for finding in &result.response.findings {
            if !is_valid_source_url(&finding.source_url) {
                continue;
            }
            let key = (
                normalize_url(&finding.source_url),
                finding.claim.trim().to_lowercase(),
            );
            if seen.insert(key) {
                merged.findings.push(finding.clone());
            }
        }
    }
    merged
}

pub fn allowed_urls(merged: &MergedFindings) -> BTreeSet<String> {
    merged
        .findings
        .iter()
        .map(|finding| normalize_url(&finding.source_url))
        .collect()
}

pub fn allowed_image_urls(merged: &MergedFindings) -> BTreeSet<String> {
    merged
        .findings
        .iter()
        .map(|finding| normalize_url(&finding.image_url))
        .filter(|image_url| is_valid_source_url(image_url))
        .collect()
}

pub fn self_declared_urls(answer: &Answer) -> BTreeSet<String> {
    answer
        .sources
        .iter()
        .filter(|source| is_valid_source_url(&source.url))
        .map(|source| normalize_url(&source.url))
        .collect()
}

pub fn strip_image_blocks(mut answer: Answer) -> Answer {
    answer
        .blocks
        .retain(|block| !matches!(block, Block::Image { .. }));
    answer
}

pub fn eject_unknown_images(mut answer: Answer, allowed: &BTreeSet<String>) -> Answer {
    answer.blocks.retain(|block| match block {
        Block::Image { url, .. } => allowed.contains(&normalize_url(url)),
        _ => true,
    });
    answer
}

pub fn image_urls(answer: &Answer) -> Vec<String> {
    answer
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Image { url, .. } => Some(url.clone()),
            _ => None,
        })
        .fold(Vec::new(), |mut urls, url| {
            if !urls.contains(&url) {
                urls.push(url);
            }
            urls
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    Valid,
    Invalid(u16),
    Unreachable,
}

pub fn classify_status(code: u16) -> LinkStatus {
    match code {
        200..=399 => LinkStatus::Valid,
        _ => LinkStatus::Invalid(code),
    }
}

pub fn should_retry_with_get(code: u16) -> bool {
    matches!(code, 403 | 405 | 501)
}

pub fn renumber_sources(mut answer: Answer, allowed: &BTreeSet<String>) -> Answer {
    let valid_by_old_id = index_allowed_sources(&answer.sources, allowed);
    let first_use_order = referenced_ids_in_first_use_order(&answer.blocks, &valid_by_old_id);
    let new_id_by_old = assign_sequential_ids(&first_use_order);
    for slot in block_source_id_slots_mut(&mut answer.blocks) {
        *slot = remap_ids(slot, &new_id_by_old);
    }
    answer.sources = first_use_order
        .iter()
        .map(|old_id| relabeled_source(&valid_by_old_id[old_id], new_id_by_old[old_id]))
        .collect();
    answer
}

fn index_allowed_sources(sources: &[Source], allowed: &BTreeSet<String>) -> BTreeMap<u32, Source> {
    sources
        .iter()
        .filter(|source| allowed.contains(&normalize_url(&source.url)))
        .map(|source| (source.id, source.clone()))
        .collect()
}

fn referenced_ids_in_first_use_order(
    blocks: &[Block],
    valid_by_old_id: &BTreeMap<u32, Source>,
) -> Vec<u32> {
    let mut order = Vec::new();
    for slot in block_source_id_slots(blocks) {
        for id in slot {
            if valid_by_old_id.contains_key(id) && !order.contains(id) {
                order.push(*id);
            }
        }
    }
    order
}

fn assign_sequential_ids(first_use_order: &[u32]) -> BTreeMap<u32, u32> {
    first_use_order
        .iter()
        .enumerate()
        .map(|(index, old_id)| (*old_id, sequential_id(index)))
        .collect()
}

fn sequential_id(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

fn remap_ids(slot: &[u32], new_id_by_old: &BTreeMap<u32, u32>) -> Vec<u32> {
    let mut seen = BTreeSet::new();
    slot.iter()
        .filter_map(|old_id| new_id_by_old.get(old_id).copied())
        .filter(|new_id| seen.insert(*new_id))
        .collect()
}

fn relabeled_source(source: &Source, new_id: u32) -> Source {
    Source {
        id: new_id,
        ..source.clone()
    }
}

fn block_source_id_slots(blocks: &[Block]) -> Vec<&Vec<u32>> {
    let mut slots = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph { source_ids, .. }
            | Block::Quote { source_ids, .. }
            | Block::Table { source_ids, .. }
            | Block::Chart { source_ids, .. }
            | Block::Diagram { source_ids, .. }
            | Block::Image { source_ids, .. } => slots.push(source_ids),
            Block::List { items, .. } => {
                slots.extend(items.iter().map(|item| &item.source_ids));
            }
            Block::Heading { .. } | Block::Unknown => {}
        }
    }
    slots
}

fn block_source_id_slots_mut(blocks: &mut [Block]) -> Vec<&mut Vec<u32>> {
    let mut slots = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph { source_ids, .. }
            | Block::Quote { source_ids, .. }
            | Block::Table { source_ids, .. }
            | Block::Chart { source_ids, .. }
            | Block::Diagram { source_ids, .. }
            | Block::Image { source_ids, .. } => slots.push(source_ids),
            Block::List { items, .. } => {
                slots.extend(items.iter_mut().map(|item| &mut item.source_ids));
            }
            Block::Heading { .. } | Block::Unknown => {}
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{DiagramType, Emphasis, ListItem};

    fn finding(claim: &str, url: &str) -> Finding {
        Finding {
            claim: claim.to_string(),
            source_title: "t".to_string(),
            source_url: url.to_string(),
            lang: "en".to_string(),
            image_url: String::new(),
        }
    }

    fn finding_with_image(claim: &str, url: &str, image_url: &str) -> Finding {
        Finding {
            image_url: image_url.to_string(),
            ..finding(claim, url)
        }
    }

    fn sub_result(query: &str, findings: Vec<Finding>) -> SubResult {
        SubResult {
            query: query.to_string(),
            lang: "en".to_string(),
            response: SubSearchResponse {
                summary: format!("summary of {query}"),
                findings,
            },
        }
    }

    fn source(id: u32, url: &str) -> Source {
        Source {
            id,
            title: format!("source {id}"),
            url: url.to_string(),
            lang: "en".to_string(),
        }
    }

    #[test]
    fn normalize_url_canonicalizes_equivalent_urls() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "lowercases scheme and host",
                input: "HTTPS://Example.COM/Path",
                want: "https://example.com/Path",
            },
            Case {
                name: "strips trailing slash",
                input: "https://example.com/path/",
                want: "https://example.com/path",
            },
            Case {
                name: "strips fragment",
                input: "https://example.com/path#section",
                want: "https://example.com/path",
            },
            Case {
                name: "keeps query string",
                input: "https://example.com/path?q=1",
                want: "https://example.com/path?q=1",
            },
            Case {
                name: "host only",
                input: "https://Example.com/",
                want: "https://example.com",
            },
            Case {
                name: "path case is preserved",
                input: "https://example.com/CaseSensitive",
                want: "https://example.com/CaseSensitive",
            },
            Case {
                name: "surrounding whitespace",
                input: "  https://example.com  ",
                want: "https://example.com",
            },
        ];
        for case in cases {
            assert_eq!(normalize_url(case.input), case.want, "{}", case.name);
        }
    }

    #[test]
    fn is_valid_source_url_requires_http_scheme_and_host() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "https",
                input: "https://example.com",
                want: true,
            },
            Case {
                name: "http",
                input: "http://example.com",
                want: true,
            },
            Case {
                name: "empty",
                input: "",
                want: false,
            },
            Case {
                name: "scheme only",
                input: "https://",
                want: false,
            },
            Case {
                name: "other scheme",
                input: "ftp://example.com",
                want: false,
            },
            Case {
                name: "bare words",
                input: "example dot com",
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(is_valid_source_url(case.input), case.want, "{}", case.name);
        }
    }

    #[test]
    fn merge_deduplicates_by_normalized_url_and_claim() {
        let results = [
            sub_result(
                "q1",
                vec![
                    finding("claim a", "https://example.com/page/"),
                    finding("claim b", "https://example.com/page"),
                ],
            ),
            sub_result(
                "q2",
                vec![
                    finding("Claim A", "HTTPS://EXAMPLE.COM/page#top"),
                    finding("claim c", "https://other.org/x"),
                ],
            ),
        ];
        let merged = merge_sub_results(&results);
        let claims: Vec<&str> = merged
            .findings
            .iter()
            .map(|item| item.claim.as_str())
            .collect();
        assert_eq!(claims, vec!["claim a", "claim b", "claim c"]);
        assert_eq!(merged.summaries.len(), 2);
    }

    #[test]
    fn merge_drops_findings_without_a_usable_url() {
        let results = [sub_result(
            "q",
            vec![
                finding("kept", "https://example.com"),
                finding("dropped", "not a url"),
                finding("dropped too", ""),
            ],
        )];
        let merged = merge_sub_results(&results);
        assert_eq!(merged.findings.len(), 1);
        assert_eq!(merged.findings[0].claim, "kept");
    }

    #[test]
    fn renumber_orders_sources_by_first_use_and_drops_the_rest() {
        let answer = Answer {
            blocks: vec![
                Block::Paragraph {
                    text: "second source first".to_string(),
                    source_ids: vec![7, 3],
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "repeat and dangling".to_string(),
                    source_ids: vec![3, 99],
                    emphasis: Emphasis::None,
                },
            ],
            sources: vec![
                source(3, "https://a.example/one"),
                source(7, "https://b.example/two"),
                source(5, "https://c.example/unused"),
            ],
            ..Answer::default()
        };
        let allowed = BTreeSet::from([
            "https://a.example/one".to_string(),
            "https://b.example/two".to_string(),
            "https://c.example/unused".to_string(),
        ]);
        let renumbered = renumber_sources(answer, &allowed);
        assert_eq!(
            renumbered.blocks[0],
            Block::Paragraph {
                text: "second source first".to_string(),
                source_ids: vec![1, 2],
                emphasis: Emphasis::None,
            }
        );
        assert_eq!(
            renumbered.blocks[1],
            Block::Paragraph {
                text: "repeat and dangling".to_string(),
                source_ids: vec![2],
                emphasis: Emphasis::None,
            }
        );
        let urls: Vec<&str> = renumbered
            .sources
            .iter()
            .map(|item| item.url.as_str())
            .collect();
        assert_eq!(urls, vec!["https://b.example/two", "https://a.example/one"]);
        assert_eq!(renumbered.sources[0].id, 1);
        assert_eq!(renumbered.sources[1].id, 2);
    }

    #[test]
    fn renumber_ejects_sources_with_hallucinated_urls() {
        let answer = Answer {
            blocks: vec![Block::Paragraph {
                text: "claims".to_string(),
                source_ids: vec![1, 2],
                emphasis: Emphasis::None,
            }],
            sources: vec![
                source(1, "https://real.example/found"),
                source(2, "https://invented.example/never-searched"),
            ],
            ..Answer::default()
        };
        let allowed = BTreeSet::from(["https://real.example/found".to_string()]);
        let renumbered = renumber_sources(answer, &allowed);
        assert_eq!(renumbered.sources.len(), 1);
        assert_eq!(renumbered.sources[0].url, "https://real.example/found");
        assert_eq!(
            renumbered.blocks[0],
            Block::Paragraph {
                text: "claims".to_string(),
                source_ids: vec![1],
                emphasis: Emphasis::None,
            }
        );
    }

    #[test]
    fn renumber_handles_list_items_and_duplicate_ids() {
        let answer = Answer {
            blocks: vec![Block::List {
                ordered: false,
                items: vec![
                    ListItem {
                        text: "one".to_string(),
                        source_ids: vec![4, 4, 9],
                    },
                    ListItem {
                        text: "two".to_string(),
                        source_ids: vec![9],
                    },
                ],
                emphasis: Emphasis::None,
            }],
            sources: vec![
                source(4, "https://a.example"),
                source(9, "https://b.example"),
            ],
            ..Answer::default()
        };
        let allowed = BTreeSet::from([
            "https://a.example".to_string(),
            "https://b.example".to_string(),
        ]);
        let renumbered = renumber_sources(answer, &allowed);
        let Block::List { items, .. } = &renumbered.blocks[0] else {
            panic!("expected list block");
        };
        assert_eq!(items[0].source_ids, vec![1, 2]);
        assert_eq!(items[1].source_ids, vec![2]);
    }

    #[test]
    fn renumber_remaps_diagram_source_ids() {
        let answer = Answer {
            blocks: vec![Block::Diagram {
                diagram_type: DiagramType::Flow,
                title: "d".to_string(),
                items: vec![],
                source_ids: vec![9, 4],
                emphasis: Emphasis::None,
            }],
            sources: vec![
                source(4, "https://a.example"),
                source(9, "https://b.example"),
            ],
            ..Answer::default()
        };
        let allowed = BTreeSet::from([
            "https://a.example".to_string(),
            "https://b.example".to_string(),
        ]);
        let renumbered = renumber_sources(answer, &allowed);
        let Block::Diagram { source_ids, .. } = &renumbered.blocks[0] else {
            panic!("expected a diagram block");
        };
        assert_eq!(source_ids, &vec![1, 2]);
        assert_eq!(renumbered.sources[0].url, "https://b.example");
    }

    #[test]
    fn allowed_image_urls_collects_only_valid_finding_images() {
        let results = [sub_result(
            "q",
            vec![
                finding_with_image(
                    "c1",
                    "https://page.example/a",
                    "HTTPS://Img.Example/photo.png#frag",
                ),
                finding_with_image("c2", "https://page.example/b", "not a url"),
                finding("c3", "https://page.example/c"),
            ],
        )];
        let merged = merge_sub_results(&results);
        assert_eq!(
            allowed_image_urls(&merged),
            BTreeSet::from(["https://img.example/photo.png".to_string()])
        );
    }

    #[test]
    fn eject_unknown_images_keeps_only_findings_backed_image_blocks() {
        struct Case {
            name: &'static str,
            url: &'static str,
            want_kept: bool,
        }
        let cases = [
            Case {
                name: "image url from the findings survives",
                url: "HTTPS://IMG.EXAMPLE/photo.png",
                want_kept: true,
            },
            Case {
                name: "invented image url is ejected",
                url: "https://img.example/never-found.png",
                want_kept: false,
            },
            Case {
                name: "empty image url is ejected",
                url: "",
                want_kept: false,
            },
        ];
        let allowed = BTreeSet::from(["https://img.example/photo.png".to_string()]);
        for case in cases {
            let answer = Answer {
                blocks: vec![
                    Block::Paragraph {
                        text: "kept".to_string(),
                        source_ids: vec![1],
                        emphasis: Emphasis::None,
                    },
                    Block::Image {
                        url: case.url.to_string(),
                        caption: "c".to_string(),
                        source_ids: vec![1],
                        emphasis: Emphasis::None,
                    },
                ],
                ..Answer::default()
            };
            let ejected = eject_unknown_images(answer, &allowed);
            let want_blocks = if case.want_kept { 2 } else { 1 };
            assert_eq!(ejected.blocks.len(), want_blocks, "{}", case.name);
            assert!(
                matches!(&ejected.blocks[0], Block::Paragraph { text, .. } if text == "kept"),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn renumber_remaps_image_source_ids() {
        let answer = Answer {
            blocks: vec![Block::Image {
                url: "https://img.example/x.png".to_string(),
                caption: "c".to_string(),
                source_ids: vec![9, 4],
                emphasis: Emphasis::None,
            }],
            sources: vec![
                source(4, "https://a.example"),
                source(9, "https://b.example"),
            ],
            ..Answer::default()
        };
        let allowed = BTreeSet::from([
            "https://a.example".to_string(),
            "https://b.example".to_string(),
        ]);
        let renumbered = renumber_sources(answer, &allowed);
        let Block::Image { source_ids, .. } = &renumbered.blocks[0] else {
            panic!("expected an image block");
        };
        assert_eq!(source_ids, &vec![1, 2]);
    }

    #[test]
    fn parse_sub_response_tolerates_missing_fields() {
        struct Case {
            name: &'static str,
            input: serde_json::Value,
            want_findings: usize,
        }
        let cases = [
            Case {
                name: "complete",
                input: serde_json::json!({
                    "summary": "s",
                    "findings": [
                        {"claim": "c", "source_title": "t", "source_url": "https://x.example", "lang": "en"}
                    ]
                }),
                want_findings: 1,
            },
            Case {
                name: "missing summary",
                input: serde_json::json!({
                    "findings": [{"claim": "c", "source_url": "https://x.example"}]
                }),
                want_findings: 1,
            },
            Case {
                name: "empty object",
                input: serde_json::json!({}),
                want_findings: 0,
            },
        ];
        for case in cases {
            let parsed = parse_sub_response(&case.input);
            assert_eq!(
                parsed.map(|response| response.findings.len()),
                Some(case.want_findings),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn classify_status_maps_http_codes_to_link_health() {
        struct Case {
            name: &'static str,
            code: u16,
            want: LinkStatus,
        }
        let cases = [
            Case {
                name: "ok",
                code: 200,
                want: LinkStatus::Valid,
            },
            Case {
                name: "redirect already followed",
                code: 301,
                want: LinkStatus::Valid,
            },
            Case {
                name: "not found",
                code: 404,
                want: LinkStatus::Invalid(404),
            },
            Case {
                name: "gone",
                code: 410,
                want: LinkStatus::Invalid(410),
            },
            Case {
                name: "server error",
                code: 500,
                want: LinkStatus::Invalid(500),
            },
        ];
        for case in cases {
            assert_eq!(classify_status(case.code), case.want, "{}", case.name);
        }
    }

    #[test]
    fn head_unfriendly_codes_trigger_a_ranged_get_retry() {
        struct Case {
            name: &'static str,
            code: u16,
            want: bool,
        }
        let cases = [
            Case {
                name: "forbidden often blocks head only",
                code: 403,
                want: true,
            },
            Case {
                name: "method not allowed",
                code: 405,
                want: true,
            },
            Case {
                name: "not implemented",
                code: 501,
                want: true,
            },
            Case {
                name: "ok needs no retry",
                code: 200,
                want: false,
            },
            Case {
                name: "not found needs no retry",
                code: 404,
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(should_retry_with_get(case.code), case.want, "{}", case.name);
        }
    }

    fn image_block(url: &str) -> Block {
        Block::Image {
            url: url.to_string(),
            caption: String::new(),
            source_ids: vec![],
            emphasis: Emphasis::None,
        }
    }

    #[test]
    fn image_urls_are_unique_and_in_document_order() {
        struct Case {
            name: &'static str,
            blocks: Vec<Block>,
            want: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "no image blocks yield nothing",
                blocks: vec![Block::Unknown],
                want: vec![],
            },
            Case {
                name: "duplicates collapse keeping first position",
                blocks: vec![
                    image_block("https://img.example/a.png"),
                    Block::Unknown,
                    image_block("https://img.example/b.png"),
                    image_block("https://img.example/a.png"),
                ],
                want: vec!["https://img.example/a.png", "https://img.example/b.png"],
            },
        ];
        for case in cases {
            let answer = Answer {
                blocks: case.blocks,
                ..Answer::default()
            };
            assert_eq!(image_urls(&answer), case.want, "{}", case.name);
        }
    }
}
