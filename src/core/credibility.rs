use crate::core::answer::{Answer, Source};
use crate::core::citations::{block_source_id_slots, normalize_url};
use crate::core::websearch::{WebCategory, WebHit, engine_by_name};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    PeerReviewed,
    Institutional,
    Reference,
    Press,
    Community,
    #[default]
    Unknown,
}

#[derive(Debug)]
pub struct ClassSpec {
    pub class: SourceClass,
    pub label: &'static str,
    pub glyph: &'static str,
}

pub const SOURCE_CLASSES: &[ClassSpec] = &[
    ClassSpec {
        class: SourceClass::PeerReviewed,
        label: "peer-reviewed",
        glyph: "\u{2b22}",
    },
    ClassSpec {
        class: SourceClass::Institutional,
        label: "institutional",
        glyph: "\u{25c6}",
    },
    ClassSpec {
        class: SourceClass::Reference,
        label: "reference",
        glyph: "\u{25c7}",
    },
    ClassSpec {
        class: SourceClass::Press,
        label: "press",
        glyph: "\u{25cb}",
    },
    ClassSpec {
        class: SourceClass::Community,
        label: "community",
        glyph: "\u{25cc}",
    },
    ClassSpec {
        class: SourceClass::Unknown,
        label: "unclassified",
        glyph: "\u{b7}",
    },
];

#[derive(Debug)]
pub struct DomainRule {
    pub pattern: &'static str,
    pub class: SourceClass,
}

pub const DOMAIN_RULES: &[DomainRule] = &[
    DomainRule {
        pattern: "doi.org",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "arxiv.org",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "pubmed.ncbi.nlm.nih.gov",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "nature.com",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "science.org",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "sciencedirect.com",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "springer.com",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: "biorxiv.org",
        class: SourceClass::PeerReviewed,
    },
    DomainRule {
        pattern: ".edu",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: ".gov",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: ".ac.uk",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: ".int",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: "who.int",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: "europa.eu",
        class: SourceClass::Institutional,
    },
    DomainRule {
        pattern: "wikipedia.org",
        class: SourceClass::Reference,
    },
    DomainRule {
        pattern: "docs.rs",
        class: SourceClass::Reference,
    },
    DomainRule {
        pattern: "developer.mozilla.org",
        class: SourceClass::Reference,
    },
    DomainRule {
        pattern: "reuters.com",
        class: SourceClass::Press,
    },
    DomainRule {
        pattern: "apnews.com",
        class: SourceClass::Press,
    },
    DomainRule {
        pattern: "bbc.co",
        class: SourceClass::Press,
    },
    DomainRule {
        pattern: "nytimes.com",
        class: SourceClass::Press,
    },
    DomainRule {
        pattern: "theguardian.com",
        class: SourceClass::Press,
    },
    DomainRule {
        pattern: "reddit.com",
        class: SourceClass::Community,
    },
    DomainRule {
        pattern: "news.ycombinator.com",
        class: SourceClass::Community,
    },
    DomainRule {
        pattern: "stackoverflow.com",
        class: SourceClass::Community,
    },
    DomainRule {
        pattern: "lobste.rs",
        class: SourceClass::Community,
    },
    DomainRule {
        pattern: "medium.com",
        class: SourceClass::Community,
    },
    DomainRule {
        pattern: "substack.com",
        class: SourceClass::Community,
    },
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceMeta {
    pub published: Option<u16>,
    pub peer_reviewed: bool,
}

impl SourceClass {
    pub fn spec(self) -> &'static ClassSpec {
        SOURCE_CLASSES
            .iter()
            .find(|spec| spec.class == self)
            .expect("SOURCE_CLASSES holds one row per SourceClass variant")
    }

    pub fn label(self) -> &'static str {
        self.spec().label
    }

    pub fn glyph(self) -> &'static str {
        self.spec().glyph
    }
}

pub fn classify(url: &str, meta: Option<&SourceMeta>) -> SourceClass {
    if meta.is_some_and(|meta| meta.peer_reviewed) {
        return SourceClass::PeerReviewed;
    }
    let host = host_of(url);
    DOMAIN_RULES
        .iter()
        .find(|rule| matches_host(&host, rule.pattern))
        .map_or(SourceClass::Unknown, |rule| rule.class)
}

fn host_of(url: &str) -> String {
    let after_scheme = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_start_matches("www.");
    after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn matches_host(host: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('.') {
        return host.ends_with(&format!(".{suffix}")) || host == suffix;
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

pub fn meta_by_url(hits: &[WebHit]) -> BTreeMap<String, SourceMeta> {
    hits.iter().fold(BTreeMap::new(), |mut map, hit| {
        let entry = map.entry(normalize_url(&hit.url)).or_default();
        let meta: &mut SourceMeta = entry;
        meta.published = meta.published.or(hit.published);
        meta.peer_reviewed |= is_academic(hit.engine);
        map
    })
}

fn is_academic(engine: &str) -> bool {
    engine_by_name(engine).is_some_and(|spec| spec.category == WebCategory::Academic)
}

pub fn annotate_sources(mut answer: Answer, hits: &[WebHit]) -> Answer {
    let meta = meta_by_url(hits);
    for source in &mut answer.sources {
        let found = meta.get(&normalize_url(&source.url));
        source.published = found.and_then(|meta| meta.published);
        source.class = classify(&source.url, found);
    }
    answer
}

pub fn sole_support_sources(answer: &Answer) -> BTreeSet<u32> {
    block_source_id_slots(&answer.blocks)
        .iter()
        .filter(|slot| slot.len() == 1)
        .filter_map(|slot| slot.first().copied())
        .collect()
}

pub fn source_badge(source: &Source) -> String {
    match source.published {
        Some(year) => format!("{} {year}", source.class.glyph()),
        None => source.class.glyph().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{Block, Emphasis, ListItem};

    fn hit(url: &str, engine: &'static str, published: Option<u16>) -> WebHit {
        WebHit {
            title: "t".to_string(),
            url: url.to_string(),
            snippet: "s".to_string(),
            engine,
            published,
        }
    }

    fn source(id: u32, url: &str) -> Source {
        Source {
            id,
            title: format!("s{id}"),
            url: url.to_string(),
            lang: "en".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn source_classes_table_covers_every_variant_uniquely() {
        let labels: BTreeSet<&str> = SOURCE_CLASSES.iter().map(|spec| spec.label).collect();
        let glyphs: BTreeSet<&str> = SOURCE_CLASSES.iter().map(|spec| spec.glyph).collect();
        assert_eq!(labels.len(), SOURCE_CLASSES.len());
        assert_eq!(glyphs.len(), SOURCE_CLASSES.len());
        for spec in SOURCE_CLASSES {
            assert_eq!(spec.class.spec().class, spec.class, "{}", spec.label);
        }
    }

    #[test]
    fn domain_rules_classify_by_host_not_by_substring() {
        struct Case {
            name: &'static str,
            url: &'static str,
            want: SourceClass,
        }
        let cases = [
            Case {
                name: "a doi resolves as peer-reviewed",
                url: "https://doi.org/10.1234/abc",
                want: SourceClass::PeerReviewed,
            },
            Case {
                name: "a university is institutional",
                url: "https://cs.stanford.edu/research",
                want: SourceClass::Institutional,
            },
            Case {
                name: "a government site is institutional",
                url: "https://www.nih.gov/news",
                want: SourceClass::Institutional,
            },
            Case {
                name: "wikipedia is reference",
                url: "https://en.wikipedia.org/wiki/Rust",
                want: SourceClass::Reference,
            },
            Case {
                name: "a wire service is press",
                url: "https://www.reuters.com/world/story",
                want: SourceClass::Press,
            },
            Case {
                name: "a forum is community",
                url: "https://stackoverflow.com/questions/1",
                want: SourceClass::Community,
            },
            Case {
                name: "an unknown host stays unclassified",
                url: "https://example.com/page",
                want: SourceClass::Unknown,
            },
            Case {
                name: "the pattern must match the host, not the path",
                url: "https://example.com/wikipedia.org/fake",
                want: SourceClass::Unknown,
            },
            Case {
                name: "a lookalike host does not match",
                url: "https://notreddit.com/r/rust",
                want: SourceClass::Unknown,
            },
            Case {
                name: "a subdomain of a known host still matches",
                url: "https://old.reddit.com/r/rust",
                want: SourceClass::Community,
            },
            Case {
                name: "a suffix rule does not swallow a lookalike",
                url: "https://notedu.com/x",
                want: SourceClass::Unknown,
            },
        ];
        for case in cases {
            assert_eq!(classify(case.url, None), case.want, "{}", case.name);
        }
    }

    #[test]
    fn an_academic_engine_outranks_the_domain_rules() {
        let academic = SourceMeta {
            published: Some(2024),
            peer_reviewed: true,
        };
        assert_eq!(
            classify("https://example.com/paper.pdf", Some(&academic)),
            SourceClass::PeerReviewed
        );
        assert_eq!(
            classify("https://example.com/paper.pdf", None),
            SourceClass::Unknown
        );
    }

    #[test]
    fn metadata_is_keyed_by_normalized_url_and_merges_across_engines() {
        let hits = [
            hit("https://Example.com/Paper/", "ddg", None),
            hit("https://example.com/Paper", "openalex", Some(2019)),
        ];
        let meta = meta_by_url(&hits);
        assert_eq!(meta.len(), 1);
        let found = meta.get("https://example.com/Paper").unwrap();
        assert_eq!(found.published, Some(2019));
        assert!(found.peer_reviewed);
    }

    #[test]
    fn annotation_fills_class_and_year_from_the_hits() {
        let answer = Answer {
            sources: vec![
                source(1, "https://doi.org/10.1/x"),
                source(2, "https://example.com/blog"),
            ],
            ..Answer::default()
        };
        let hits = [hit("https://doi.org/10.1/x", "crossref", Some(2021))];
        let annotated = annotate_sources(answer, &hits);
        assert_eq!(annotated.sources[0].class, SourceClass::PeerReviewed);
        assert_eq!(annotated.sources[0].published, Some(2021));
        assert_eq!(annotated.sources[1].class, SourceClass::Unknown);
        assert_eq!(annotated.sources[1].published, None);
    }

    #[test]
    fn sole_support_finds_sources_no_other_source_corroborates() {
        let answer = Answer {
            blocks: vec![
                Block::Paragraph {
                    text: "corroborated".to_string(),
                    source_ids: vec![1, 2],
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "stands alone".to_string(),
                    source_ids: vec![3],
                    emphasis: Emphasis::None,
                },
                Block::List {
                    ordered: false,
                    items: vec![ListItem {
                        text: "also alone".to_string(),
                        source_ids: vec![4],
                    }],
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "uncited".to_string(),
                    source_ids: vec![],
                    emphasis: Emphasis::None,
                },
            ],
            ..Answer::default()
        };
        assert_eq!(sole_support_sources(&answer), BTreeSet::from([3, 4]));
    }

    #[test]
    fn the_badge_shows_the_year_only_when_one_is_known() {
        let mut dated = source(1, "https://doi.org/10.1/x");
        dated.class = SourceClass::PeerReviewed;
        dated.published = Some(2024);
        assert_eq!(source_badge(&dated), "\u{2b22} 2024");

        let mut undated = source(2, "https://example.com");
        undated.class = SourceClass::Unknown;
        assert_eq!(source_badge(&undated), "\u{b7}");
    }
}
