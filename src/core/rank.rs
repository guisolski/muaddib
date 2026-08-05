use crate::core::websearch::WebHit;
use std::cmp::Ordering;
use std::collections::BTreeSet;

pub const RANK_POOL_FACTOR: usize = 3;
const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;

pub fn pool_budget(budget: usize) -> usize {
    budget.saturating_mul(RANK_POOL_FACTOR)
}

pub fn rank_hits(query: &str, hits: &[WebHit], limit: usize) -> Vec<WebHit> {
    let terms = unique_tokens(query);
    if terms.is_empty() || hits.len() < 2 {
        return hits.iter().take(limit).cloned().collect();
    }
    let docs: Vec<Vec<String>> = hits.iter().map(hit_tokens).collect();
    let avg_len = (docs.iter().map(Vec::len).sum::<usize>() as f64 / docs.len() as f64).max(1.0);
    let scores: Vec<f64> = docs
        .iter()
        .map(|doc| bm25_score(&terms, doc, &docs, avg_len))
        .collect();
    let mut order: Vec<usize> = (0..hits.len()).collect();
    order.sort_by(|left, right| {
        scores[*right]
            .partial_cmp(&scores[*left])
            .unwrap_or(Ordering::Equal)
    });
    order
        .into_iter()
        .take(limit)
        .map(|idx| hits[idx].clone())
        .collect()
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn unique_tokens(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    tokenize(text)
        .into_iter()
        .filter(|token| seen.insert(token.clone()))
        .collect()
}

fn hit_tokens(hit: &WebHit) -> Vec<String> {
    let title = tokenize(&hit.title);
    [title.clone(), title, tokenize(&hit.snippet)].concat()
}

fn bm25_score(terms: &[String], doc: &[String], docs: &[Vec<String>], avg_len: f64) -> f64 {
    let len_norm = BM25_K1 * (1.0 - BM25_B + BM25_B * doc.len() as f64 / avg_len);
    terms
        .iter()
        .map(|term| term_score(term, doc, docs, len_norm))
        .sum()
}

fn term_score(term: &str, doc: &[String], docs: &[Vec<String>], len_norm: f64) -> f64 {
    let count = doc.iter().filter(|token| token.as_str() == term).count();
    if count == 0 {
        return 0.0;
    }
    let tf = count as f64;
    idf(term, docs) * tf * (BM25_K1 + 1.0) / (tf + len_norm)
}

fn idf(term: &str, docs: &[Vec<String>]) -> f64 {
    let df = docs
        .iter()
        .filter(|doc| doc.iter().any(|token| token.as_str() == term))
        .count() as f64;
    let n = docs.len() as f64;
    (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: &str, snippet: &str, url: &str) -> WebHit {
        WebHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
            engine: "ddg",
            ..Default::default()
        }
    }

    struct Case {
        name: &'static str,
        query: &'static str,
        hits: Vec<WebHit>,
        limit: usize,
        want_urls: Vec<&'static str>,
    }

    fn assert_ranked(cases: Vec<Case>) {
        for case in cases {
            let got: Vec<String> = rank_hits(case.query, &case.hits, case.limit)
                .into_iter()
                .map(|ranked| ranked.url)
                .collect();
            assert_eq!(got, case.want_urls, "{}", case.name);
        }
    }

    #[test]
    fn rank_orders_hits_by_lexical_relevance() {
        let cases = vec![
            Case {
                name: "title match outranks unrelated hit",
                query: "rust async runtime",
                hits: vec![
                    hit(
                        "Cooking pasta at home",
                        "boil water first",
                        "https://a.example",
                    ),
                    hit(
                        "Rust async runtime comparison",
                        "tokio and async-std traded off",
                        "https://b.example",
                    ),
                ],
                limit: 5,
                want_urls: vec!["https://b.example", "https://a.example"],
            },
            Case {
                name: "snippet-only match still ranks",
                query: "tokio",
                hits: vec![
                    hit(
                        "Runtime guide",
                        "nothing relevant here",
                        "https://a.example",
                    ),
                    hit(
                        "Runtime guide two",
                        "covers tokio in depth",
                        "https://b.example",
                    ),
                ],
                limit: 5,
                want_urls: vec!["https://b.example", "https://a.example"],
            },
            Case {
                name: "rare term beats a term present in every doc",
                query: "alpha beta",
                hits: vec![
                    hit("alpha guide", "", "https://a.example"),
                    hit("alpha intro", "", "https://b.example"),
                    hit("beta manual", "", "https://c.example"),
                ],
                limit: 5,
                want_urls: vec![
                    "https://c.example",
                    "https://a.example",
                    "https://b.example",
                ],
            },
            Case {
                name: "title match beats snippet match for the same term",
                query: "quantum",
                hits: vec![
                    hit("Computing basics", "quantum overview", "https://a.example"),
                    hit("Quantum computing basics", "intro", "https://b.example"),
                ],
                limit: 5,
                want_urls: vec!["https://b.example", "https://a.example"],
            },
        ];
        assert_ranked(cases);
    }

    #[test]
    fn rank_preserves_order_on_degenerate_input_and_truncates() {
        let cases = vec![
            Case {
                name: "ties keep engine input order",
                query: "zzz",
                hits: vec![
                    hit("first title", "one", "https://a.example"),
                    hit("second title", "two", "https://b.example"),
                ],
                limit: 5,
                want_urls: vec!["https://a.example", "https://b.example"],
            },
            Case {
                name: "empty query keeps input order",
                query: "",
                hits: vec![
                    hit("first", "one", "https://a.example"),
                    hit("second", "two", "https://b.example"),
                ],
                limit: 5,
                want_urls: vec!["https://a.example", "https://b.example"],
            },
            Case {
                name: "limit truncates the ranked list",
                query: "rust",
                hits: vec![
                    hit("gardening", "flowers", "https://a.example"),
                    hit("rust book", "the rust language", "https://b.example"),
                    hit("rust forum", "rust questions", "https://c.example"),
                ],
                limit: 1,
                want_urls: vec!["https://c.example"],
            },
            Case {
                name: "uppercase accented query matches lowercase doc",
                query: "CAFÉ",
                hits: vec![
                    hit("chá verde", "folhas de chá", "https://a.example"),
                    hit("café coado", "grãos de café", "https://b.example"),
                ],
                limit: 5,
                want_urls: vec!["https://b.example", "https://a.example"],
            },
        ];
        assert_ranked(cases);
    }

    #[test]
    fn pool_budget_scales_by_the_rank_factor() {
        struct Case {
            name: &'static str,
            budget: usize,
            want: usize,
        }
        let cases = [
            Case {
                name: "default budget",
                budget: 5,
                want: 15,
            },
            Case {
                name: "minimum budget",
                budget: 1,
                want: 3,
            },
            Case {
                name: "zero budget",
                budget: 0,
                want: 0,
            },
        ];
        for case in cases {
            assert_eq!(pool_budget(case.budget), case.want, "{}", case.name);
        }
    }

    #[test]
    fn single_hit_pools_skip_scoring() {
        let hits = vec![hit("only", "hit", "https://a.example")];
        assert_eq!(rank_hits("query", &hits, 5), hits);
        assert!(rank_hits("query", &[], 5).is_empty());
    }
}
