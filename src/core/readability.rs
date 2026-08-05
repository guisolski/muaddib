use crate::core::citations::normalize_url;
use crate::core::websearch::WebHit;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageText {
    pub url: String,
    pub text: String,
}

pub fn page_targets(hits: &[Vec<WebHit>], top_n: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    hits.iter()
        .flat_map(|query_hits| query_hits.iter().take(top_n))
        .filter(|hit| seen.insert(normalize_url(&hit.url)))
        .map(|hit| hit.url.clone())
        .collect()
}

pub fn assign_pages(
    hits: &[Vec<WebHit>],
    top_n: usize,
    fetched: &BTreeMap<String, String>,
) -> Vec<Vec<PageText>> {
    hits.iter()
        .map(|query_hits| {
            query_hits
                .iter()
                .take(top_n)
                .filter_map(|hit| {
                    fetched.get(&normalize_url(&hit.url)).map(|text| PageText {
                        url: hit.url.clone(),
                        text: text.clone(),
                    })
                })
                .collect()
        })
        .collect()
}

pub fn excerpt(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => format!("{}\u{2026}", &text[..byte_index]),
        None => text.to_string(),
    }
}

pub fn pages_prompt_block(pages: &[PageText]) -> String {
    if pages.is_empty() {
        return String::new();
    }
    let entries: String = pages
        .iter()
        .enumerate()
        .map(|(index, page)| format!("{}. {}\n{}\n", index + 1, page.url, page.text))
        .collect();
    format!(
        "Fetched page content from the top candidate sources (may be partial):\n\
         {entries}\
         Ground findings in this content where it answers the query, citing the page URL \
         as the source; verify anything ambiguous with your own search.\n"
    )
}

#[cfg(feature = "websearch")]
pub use extract::extract_readable_text;

#[cfg(feature = "websearch")]
mod extract {
    use scraper::{ElementRef, Html, Selector};

    const CONTENT_SELECTORS: &[&str] = &["article", "main", "[role=main]", "body"];
    const NOISE_TAGS: &[&str] = &[
        "script", "style", "nav", "header", "footer", "aside", "form", "noscript",
    ];

    pub fn extract_readable_text(html: &str) -> String {
        let document = Html::parse_document(html);
        content_root(&document)
            .map(|root| {
                let mut chunks: Vec<String> = Vec::new();
                append_text(root, &mut chunks);
                chunks
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    }

    fn content_root(document: &Html) -> Option<ElementRef<'_>> {
        CONTENT_SELECTORS.iter().find_map(|selector| {
            let parsed = Selector::parse(selector).ok()?;
            document.select(&parsed).next()
        })
    }

    fn append_text(element: ElementRef, chunks: &mut Vec<String>) {
        if NOISE_TAGS.contains(&element.value().name()) {
            return;
        }
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                chunks.push(text.to_string());
            } else if let Some(child_element) = ElementRef::wrap(child) {
                append_text(child_element, chunks);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(url: &str) -> WebHit {
        WebHit {
            title: format!("title of {url}"),
            url: url.to_string(),
            snippet: "snippet".to_string(),
            engine: "ddg",
            ..Default::default()
        }
    }

    #[test]
    fn page_targets_dedupe_across_sub_queries_and_cap_at_top_n() {
        let hits = vec![
            vec![
                hit("https://a.example"),
                hit("https://b.example"),
                hit("https://skip.example"),
            ],
            vec![hit("https://a.example/"), hit("https://c.example")],
        ];
        assert_eq!(
            page_targets(&hits, 2),
            vec![
                "https://a.example",
                "https://b.example",
                "https://c.example"
            ]
        );
    }

    #[test]
    fn assign_pages_maps_fetched_text_back_to_each_sub_query() {
        let hits = vec![
            vec![hit("https://a.example"), hit("https://miss.example")],
            vec![hit("https://a.example/")],
        ];
        let fetched =
            BTreeMap::from([(normalize_url("https://a.example"), "extracted".to_string())]);
        let pages = assign_pages(&hits, 2, &fetched);
        assert_eq!(
            pages[0],
            vec![PageText {
                url: "https://a.example".to_string(),
                text: "extracted".to_string(),
            }]
        );
        assert_eq!(pages[1][0].url, "https://a.example/");
        assert_eq!(pages[1][0].text, "extracted");
    }

    fn page(url: &str, text: &str) -> PageText {
        PageText {
            url: url.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn excerpt_truncates_on_char_boundaries() {
        struct Case {
            name: &'static str,
            text: &'static str,
            max_chars: usize,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "short text passes through",
                text: "short",
                max_chars: 10,
                want: "short",
            },
            Case {
                name: "long text gains an ellipsis",
                text: "abcdef",
                max_chars: 3,
                want: "abc\u{2026}",
            },
            Case {
                name: "multibyte chars stay intact",
                text: "águas passadas",
                max_chars: 5,
                want: "águas\u{2026}",
            },
            Case {
                name: "exact length is untouched",
                text: "abc",
                max_chars: 3,
                want: "abc",
            },
        ];
        for case in cases {
            assert_eq!(
                excerpt(case.text, case.max_chars),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn pages_prompt_block_is_empty_without_pages() {
        assert_eq!(pages_prompt_block(&[]), "");
    }

    #[test]
    fn pages_prompt_block_numbers_pages_and_instructs_grounding() {
        let block = pages_prompt_block(&[
            page("https://a.example", "first body"),
            page("https://b.example", "second body"),
        ]);
        assert!(block.contains("1. https://a.example\nfirst body"));
        assert!(block.contains("2. https://b.example\nsecond body"));
        assert!(block.contains("citing the page URL"));
    }
}

#[cfg(all(test, feature = "websearch"))]
mod extract_tests {
    use super::extract_readable_text;

    const ARTICLE_SEMANTIC: &str =
        include_str!("../../tests/fixtures/websearch/article_semantic.html");
    const ARTICLE_MAIN_ROLE: &str =
        include_str!("../../tests/fixtures/websearch/article_main_role.html");
    const ARTICLE_BARE: &str = include_str!("../../tests/fixtures/websearch/article_bare.html");
    const ARTICLE_NOISE_ONLY: &str =
        include_str!("../../tests/fixtures/websearch/article_noise_only.html");

    #[test]
    fn extraction_prefers_content_roots_and_drops_noise() {
        struct Case {
            name: &'static str,
            html: &'static str,
            want_contains: &'static [&'static str],
            want_absent: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "article element wins over surrounding chrome",
                html: ARTICLE_SEMANTIC,
                want_contains: &["Desert ecology studies", "annual rainfall below 250mm"],
                want_absent: &[
                    "Site navigation",
                    "All rights reserved",
                    "trackVisit",
                    "Related links sidebar",
                ],
            },
            Case {
                name: "role main is honored when no article exists",
                html: ARTICLE_MAIN_ROLE,
                want_contains: &["Kangaroo mice survive without drinking water"],
                want_absent: &["Cookie banner"],
            },
            Case {
                name: "body fallback still drops nested noise",
                html: ARTICLE_BARE,
                want_contains: &["Plain page paragraph"],
                want_absent: &["var analytics", "Footer small print"],
            },
            Case {
                name: "noise-only page yields empty text",
                html: ARTICLE_NOISE_ONLY,
                want_contains: &[],
                want_absent: &["window.load", "Menu"],
            },
        ];
        for case in cases {
            let text = extract_readable_text(case.html);
            for needle in case.want_contains {
                assert!(
                    text.contains(needle),
                    "{}: missing {needle:?} in {text:?}",
                    case.name
                );
            }
            for needle in case.want_absent {
                assert!(
                    !text.contains(needle),
                    "{}: unexpected {needle:?} in {text:?}",
                    case.name
                );
            }
        }
    }

    #[test]
    fn extraction_normalizes_whitespace() {
        let text = extract_readable_text(ARTICLE_SEMANTIC);
        assert!(!text.contains('\n'), "{text:?}");
        assert!(!text.contains("  "), "{text:?}");
    }

    #[test]
    fn noise_only_page_extracts_nothing() {
        assert_eq!(extract_readable_text(ARTICLE_NOISE_ONLY), "");
    }
}
