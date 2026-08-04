use crate::core::citations::{
    Finding, MergedFindings, SubResult, SubSearchResponse, allowed_urls, is_valid_source_url,
    normalize_url,
};
use crate::core::config::WebSearchConfig;
use crate::core::mode::Mode;
use crate::core::plan::SubQuery;
use std::collections::BTreeSet;

pub const SNIPPET_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebEngineId {
    DuckDuckGoHtml,
    DuckDuckGoLite,
    Bing,
    Mojeek,
    Google,
    OpenAlex,
    Crossref,
    SemanticScholar,
    SearxNg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebCategory {
    Web,
    Academic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestShape {
    Get,
    PostForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitParser {
    DdgHtml,
    DdgLite,
    BingHtml,
    MojeekHtml,
    GoogleHtml,
    OpenAlexJson,
    CrossrefJson,
    SemanticScholarJson,
    SearxNgJson,
}

#[derive(Debug)]
pub struct WebEngineSpec {
    pub id: WebEngineId,
    pub name: &'static str,
    pub category: WebCategory,
    pub shape: RequestShape,
    pub url: &'static str,
    pub query_param: &'static str,
    pub extra_params: &'static [(&'static str, &'static str)],
    pub supports_mailto: bool,
    pub url_from_config: bool,
    pub parser: HitParser,
    pub fallback: Option<WebEngineId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub engine: &'static str,
}

pub const WEB_ENGINES: &[WebEngineSpec] = &[
    WebEngineSpec {
        id: WebEngineId::DuckDuckGoHtml,
        name: "ddg",
        category: WebCategory::Web,
        shape: RequestShape::PostForm,
        url: "https://html.duckduckgo.com/html/",
        query_param: "q",
        extra_params: &[("b", "")],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::DdgHtml,
        fallback: Some(WebEngineId::DuckDuckGoLite),
    },
    WebEngineSpec {
        id: WebEngineId::DuckDuckGoLite,
        name: "ddg-lite",
        category: WebCategory::Web,
        shape: RequestShape::PostForm,
        url: "https://lite.duckduckgo.com/lite/",
        query_param: "q",
        extra_params: &[],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::DdgLite,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::Bing,
        name: "bing",
        category: WebCategory::Web,
        shape: RequestShape::Get,
        url: "https://www.bing.com/search",
        query_param: "q",
        extra_params: &[],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::BingHtml,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::Mojeek,
        name: "mojeek",
        category: WebCategory::Web,
        shape: RequestShape::Get,
        url: "https://www.mojeek.com/search",
        query_param: "q",
        extra_params: &[],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::MojeekHtml,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::Google,
        name: "google",
        category: WebCategory::Web,
        shape: RequestShape::Get,
        url: "https://www.google.com/search",
        query_param: "q",
        extra_params: &[("num", "10"), ("hl", "en")],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::GoogleHtml,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::OpenAlex,
        name: "openalex",
        category: WebCategory::Academic,
        shape: RequestShape::Get,
        url: "https://api.openalex.org/works",
        query_param: "search",
        extra_params: &[("per-page", "10")],
        supports_mailto: true,
        url_from_config: false,
        parser: HitParser::OpenAlexJson,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::Crossref,
        name: "crossref",
        category: WebCategory::Academic,
        shape: RequestShape::Get,
        url: "https://api.crossref.org/works",
        query_param: "query",
        extra_params: &[("rows", "10")],
        supports_mailto: true,
        url_from_config: false,
        parser: HitParser::CrossrefJson,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::SemanticScholar,
        name: "s2",
        category: WebCategory::Academic,
        shape: RequestShape::Get,
        url: "https://api.semanticscholar.org/graph/v1/paper/search",
        query_param: "query",
        extra_params: &[
            ("limit", "10"),
            ("fields", "title,abstract,url,year,venue,externalIds"),
        ],
        supports_mailto: false,
        url_from_config: false,
        parser: HitParser::SemanticScholarJson,
        fallback: None,
    },
    WebEngineSpec {
        id: WebEngineId::SearxNg,
        name: "searxng",
        category: WebCategory::Web,
        shape: RequestShape::Get,
        url: "",
        query_param: "q",
        extra_params: &[("format", "json")],
        supports_mailto: false,
        url_from_config: true,
        parser: HitParser::SearxNgJson,
        fallback: None,
    },
];

const MODE_WEB_ENGINES: &[(Mode, &[WebEngineId])] = &[
    (
        Mode::General,
        &[WebEngineId::DuckDuckGoHtml, WebEngineId::Bing],
    ),
    (
        Mode::Scientific,
        &[
            WebEngineId::OpenAlex,
            WebEngineId::Crossref,
            WebEngineId::SemanticScholar,
            WebEngineId::DuckDuckGoHtml,
        ],
    ),
    (
        Mode::News,
        &[WebEngineId::DuckDuckGoHtml, WebEngineId::Bing],
    ),
    (
        Mode::Deep,
        &[
            WebEngineId::DuckDuckGoHtml,
            WebEngineId::Bing,
            WebEngineId::Mojeek,
        ],
    ),
];

pub fn engine_by_name(name: &str) -> Option<&'static WebEngineSpec> {
    WEB_ENGINES.iter().find(|spec| spec.name == name)
}

pub fn engine_by_id(id: WebEngineId) -> Option<&'static WebEngineSpec> {
    WEB_ENGINES.iter().find(|spec| spec.id == id)
}

pub fn fallback_engine(spec: &WebEngineSpec) -> Option<&'static WebEngineSpec> {
    spec.fallback.and_then(engine_by_id)
}

pub fn engines_for_mode(mode: Mode, config: &WebSearchConfig) -> Vec<&'static WebEngineSpec> {
    if config.engines.is_empty() {
        with_configured_searxng(default_engines_for_mode(mode), config)
    } else {
        config
            .engines
            .iter()
            .filter_map(|name| engine_by_name(name))
            .collect()
    }
}

fn with_configured_searxng(
    defaults: Vec<&'static WebEngineSpec>,
    config: &WebSearchConfig,
) -> Vec<&'static WebEngineSpec> {
    let searxng = engine_by_id(WebEngineId::SearxNg)
        .filter(|spec| resolve_url(spec, config).is_some())
        .filter(|spec| !defaults.iter().any(|other| other.id == spec.id));
    searxng.into_iter().chain(defaults).collect()
}

pub fn resolve_url(spec: &WebEngineSpec, config: &WebSearchConfig) -> Option<String> {
    if !spec.url_from_config {
        return Some(spec.url.to_string());
    }
    let base = config.searxng_url.trim().trim_end_matches('/');
    (!base.is_empty()).then(|| format!("{base}/search"))
}

fn default_engines_for_mode(mode: Mode) -> Vec<&'static WebEngineSpec> {
    MODE_WEB_ENGINES
        .iter()
        .find(|(candidate, _)| *candidate == mode)
        .map(|(_, ids)| ids.iter().copied().filter_map(engine_by_id).collect())
        .unwrap_or_default()
}

pub fn page_grounding_enabled(mode: Mode, config: &WebSearchConfig) -> bool {
    config.enabled
        && config
            .ground_modes
            .iter()
            .any(|name| name.eq_ignore_ascii_case(mode.label()))
}

pub fn request_params(spec: &WebEngineSpec, query: &str, mailto: &str) -> Vec<(String, String)> {
    let base = std::iter::once((spec.query_param.to_string(), query.to_string()));
    let extras = spec
        .extra_params
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()));
    let polite = (spec.supports_mailto && !mailto.trim().is_empty())
        .then(|| ("mailto".to_string(), mailto.trim().to_string()));
    base.chain(extras).chain(polite).collect()
}

pub fn encoded_params(spec: &WebEngineSpec, query: &str, mailto: &str) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in request_params(spec, query, mailto) {
        serializer.append_pair(&key, &value);
    }
    serializer.finish()
}

pub fn hits_prompt_block(hits: &[WebHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let leads: String = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| hit_lead(index, hit))
        .collect();
    format!(
        "Candidate sources found by conventional search engines, as leads:\n\
         {leads}\
         Open the most promising candidates first and verify them before citing; \
         ignore irrelevant ones; you may also consult pages beyond this list.\n"
    )
}

fn hit_lead(index: usize, hit: &WebHit) -> String {
    let head = format!("{}. {} \u{2014} {}\n", index + 1, hit.title, hit.url);
    if hit.snippet.is_empty() {
        head
    } else {
        format!("{head}   {}\n", hit.snippet)
    }
}

pub fn hits_as_findings(hits: &[WebHit], lang: &str) -> Vec<Finding> {
    hits.iter()
        .filter(|hit| is_valid_source_url(&hit.url))
        .map(|hit| Finding {
            claim: if hit.snippet.is_empty() {
                hit.title.clone()
            } else {
                hit.snippet.clone()
            },
            source_title: hit.title.clone(),
            source_url: hit.url.clone(),
            lang: lang.to_string(),
            image_url: String::new(),
        })
        .collect()
}

pub fn snippet_sub_results(sub_queries: &[SubQuery], hits: &[Vec<WebHit>]) -> Vec<SubResult> {
    sub_queries
        .iter()
        .zip(hits)
        .map(|(sub, query_hits)| SubResult {
            query: sub.query.clone(),
            lang: sub.lang.clone(),
            response: SubSearchResponse {
                summary: String::new(),
                findings: hits_as_findings(query_hits, &sub.lang),
            },
        })
        .collect()
}

pub fn allowed_urls_with_hits(merged: &MergedFindings, hits: &[WebHit]) -> BTreeSet<String> {
    let hit_urls = hits
        .iter()
        .filter(|hit| is_valid_source_url(&hit.url))
        .map(|hit| normalize_url(&hit.url));
    allowed_urls(merged).into_iter().chain(hit_urls).collect()
}

pub fn decode_ddg_redirect(href: &str) -> Option<String> {
    query_string_param(href, "uddg").or_else(|| direct_http_url(href))
}

pub fn decode_bing_redirect(href: &str) -> Option<String> {
    if href.contains("bing.com/ck/") {
        let packed = query_string_param(href, "u")?;
        let encoded = packed.get(2..)?;
        let bytes = base64url_decode(encoded)?;
        let url = String::from_utf8(bytes).ok()?;
        direct_http_url(&url)
    } else {
        direct_http_url(href)
    }
}

pub fn decode_google_redirect(href: &str) -> Option<String> {
    if href.starts_with("/url?") {
        query_string_param(href, "q").and_then(|url| direct_http_url(&url))
    } else {
        direct_http_url(href)
    }
}

fn query_string_param(href: &str, param: &str) -> Option<String> {
    let (_, query) = href.split_once('?')?;
    form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == param)
        .map(|(_, value)| value.into_owned())
}

fn direct_http_url(href: &str) -> Option<String> {
    (href.starts_with("https://") || href.starts_with("http://")).then(|| href.to_string())
}

fn base64url_digit(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let digits = input
        .trim_end_matches('=')
        .bytes()
        .map(base64url_digit)
        .collect::<Option<Vec<u32>>>()?;
    digits.chunks(4).try_fold(Vec::new(), |mut bytes, chunk| {
        if chunk.len() < 2 {
            return None;
        }
        let packed =
            chunk.iter().fold(0u32, |acc, digit| (acc << 6) | digit) << (6 * (4 - chunk.len()));
        let unpacked = [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8];
        bytes.extend_from_slice(&unpacked[..chunk.len() - 1]);
        Some(bytes)
    })
}

#[cfg(feature = "websearch")]
pub use parse::parse_hits;

#[cfg(feature = "websearch")]
mod parse {
    use super::{
        HitParser, SNIPPET_MAX_CHARS, WebEngineSpec, WebHit, decode_bing_redirect,
        decode_ddg_redirect, decode_google_redirect, direct_http_url,
    };
    use crate::core::citations::{is_valid_source_url, normalize_url};
    use serde_json::Value;
    use std::collections::BTreeSet;

    struct RawHit {
        title: String,
        url: String,
        snippet: String,
    }

    pub fn parse_hits(spec: &'static WebEngineSpec, body: &str, max: usize) -> Vec<WebHit> {
        let raw = match spec.parser {
            HitParser::DdgHtml => parse_ddg_html(body),
            HitParser::DdgLite => parse_ddg_lite(body),
            HitParser::BingHtml => parse_bing(body),
            HitParser::MojeekHtml => parse_mojeek(body),
            HitParser::GoogleHtml => parse_google(body),
            HitParser::OpenAlexJson => parse_openalex(body),
            HitParser::CrossrefJson => parse_crossref(body),
            HitParser::SemanticScholarJson => parse_semantic_scholar(body),
            HitParser::SearxNgJson => parse_searxng(body),
        };
        assemble(raw, spec.name, max)
    }

    fn assemble(raw: Vec<RawHit>, engine: &'static str, max: usize) -> Vec<WebHit> {
        let mut seen = BTreeSet::new();
        raw.into_iter()
            .filter(|hit| is_valid_source_url(&hit.url) && !hit.title.is_empty())
            .filter(|hit| seen.insert(normalize_url(&hit.url)))
            .take(max)
            .map(|hit| WebHit {
                title: hit.title,
                url: hit.url,
                snippet: truncate_snippet(&hit.snippet),
                engine,
            })
            .collect()
    }

    fn truncate_snippet(snippet: &str) -> String {
        match snippet.char_indices().nth(SNIPPET_MAX_CHARS) {
            Some((byte_index, _)) => format!("{}\u{2026}", &snippet[..byte_index]),
            None => snippet.to_string(),
        }
    }

    fn css(selector: &str) -> Option<scraper::Selector> {
        scraper::Selector::parse(selector).ok()
    }

    fn first_element<'a>(
        scope: &scraper::ElementRef<'a>,
        selector: &str,
    ) -> Option<scraper::ElementRef<'a>> {
        scope.select(&css(selector)?).next()
    }

    fn element_text(element: &scraper::ElementRef) -> String {
        element
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn container_hits(
        body: &str,
        container_css: &str,
        extract: impl Fn(&scraper::ElementRef) -> Option<RawHit>,
    ) -> Vec<RawHit> {
        let Some(container) = css(container_css) else {
            return Vec::new();
        };
        let document = scraper::Html::parse_document(body);
        document
            .select(&container)
            .filter_map(|element| extract(&element))
            .collect()
    }

    fn linked_hit(
        result: &scraper::ElementRef,
        link_css: &str,
        snippet_css: &str,
        decode: fn(&str) -> Option<String>,
    ) -> Option<RawHit> {
        let link = first_element(result, link_css)?;
        let url = decode(link.value().attr("href")?)?;
        let snippet = first_element(result, snippet_css)
            .map(|element| element_text(&element))
            .unwrap_or_default();
        Some(RawHit {
            title: element_text(&link),
            url,
            snippet,
        })
    }

    fn parse_ddg_html(body: &str) -> Vec<RawHit> {
        container_hits(body, "div.result", |result| {
            linked_hit(
                result,
                "a.result__a",
                "a.result__snippet, div.result__snippet",
                decode_ddg_redirect,
            )
        })
    }

    fn parse_ddg_lite(body: &str) -> Vec<RawHit> {
        let Some((link_sel, snippet_sel)) = css("a.result-link").zip(css("td.result-snippet"))
        else {
            return Vec::new();
        };
        let document = scraper::Html::parse_document(body);
        let snippets: Vec<String> = document
            .select(&snippet_sel)
            .map(|element| element_text(&element))
            .collect();
        document
            .select(&link_sel)
            .enumerate()
            .filter_map(|(index, link)| {
                let url = decode_ddg_redirect(link.value().attr("href")?)?;
                Some(RawHit {
                    title: element_text(&link),
                    url,
                    snippet: snippets.get(index).cloned().unwrap_or_default(),
                })
            })
            .collect()
    }

    fn parse_bing(body: &str) -> Vec<RawHit> {
        container_hits(body, "li.b_algo", |result| {
            linked_hit(result, "h2 a", "div.b_caption p", decode_bing_redirect)
        })
    }

    fn parse_mojeek(body: &str) -> Vec<RawHit> {
        container_hits(body, "ul.results-standard li", |result| {
            linked_hit(result, "a.title", "p.s", direct_http_url)
        })
    }

    fn parse_google(body: &str) -> Vec<RawHit> {
        container_hits(body, "#search div.g", |result| {
            let link = anchor_with_heading(result)?;
            let url = decode_google_redirect(link.value().attr("href")?)?;
            let title = first_element(&link, "h3").map(|heading| element_text(&heading))?;
            let snippet = first_element(result, "div.VwiC3b")
                .map(|element| element_text(&element))
                .unwrap_or_default();
            Some(RawHit {
                title,
                url,
                snippet,
            })
        })
    }

    fn anchor_with_heading<'a>(
        result: &scraper::ElementRef<'a>,
    ) -> Option<scraper::ElementRef<'a>> {
        let anchors = css("a[href]")?;
        let heading = css("h3")?;
        result
            .select(&anchors)
            .find(|anchor| anchor.select(&heading).next().is_some())
    }

    fn json_array_hits(
        body: &str,
        pointer: &str,
        extract: fn(&Value) -> Option<RawHit>,
    ) -> Vec<RawHit> {
        let Ok(value) = serde_json::from_str::<Value>(body) else {
            return Vec::new();
        };
        value
            .pointer(pointer)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(extract).collect())
            .unwrap_or_default()
    }

    fn parse_searxng(body: &str) -> Vec<RawHit> {
        json_array_hits(body, "/results", searxng_hit)
    }

    fn searxng_hit(item: &Value) -> Option<RawHit> {
        Some(RawHit {
            title: non_empty_str(item.get("title"))?,
            url: non_empty_str(item.get("url"))?,
            snippet: non_empty_str(item.get("content")).unwrap_or_default(),
        })
    }

    fn parse_openalex(body: &str) -> Vec<RawHit> {
        json_array_hits(body, "/results", openalex_hit)
    }

    fn openalex_hit(item: &Value) -> Option<RawHit> {
        let title = non_empty_str(item.get("display_name"))?;
        let url = non_empty_str(item.get("doi"))
            .or_else(|| non_empty_str(item.pointer("/primary_location/landing_page_url")))?;
        let venue = non_empty_str(item.pointer("/primary_location/source/display_name"));
        let year = json_year(item.get("publication_year"));
        let authors = item
            .get("authorships")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|entry| non_empty_str(entry.pointer("/author/display_name")))
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(", ")
            });
        Some(RawHit {
            title,
            url,
            snippet: academic_snippet(venue, year, authors),
        })
    }

    fn parse_crossref(body: &str) -> Vec<RawHit> {
        json_array_hits(body, "/message/items", crossref_hit)
    }

    fn crossref_hit(item: &Value) -> Option<RawHit> {
        let title = non_empty_str(item.pointer("/title/0"))?;
        let url = non_empty_str(item.get("URL"))?;
        let venue = non_empty_str(item.pointer("/container-title/0"));
        let year = json_year(item.pointer("/issued/date-parts/0/0"));
        let authors = item.get("author").and_then(Value::as_array).map(|list| {
            list.iter()
                .filter_map(crossref_author)
                .take(3)
                .collect::<Vec<_>>()
                .join(", ")
        });
        Some(RawHit {
            title,
            url,
            snippet: academic_snippet(venue, year, authors),
        })
    }

    fn crossref_author(author: &Value) -> Option<String> {
        let family = non_empty_str(author.get("family"))?;
        Some(match non_empty_str(author.get("given")) {
            Some(given) => format!("{given} {family}"),
            None => family,
        })
    }

    fn parse_semantic_scholar(body: &str) -> Vec<RawHit> {
        json_array_hits(body, "/data", semantic_scholar_hit)
    }

    fn semantic_scholar_hit(item: &Value) -> Option<RawHit> {
        let title = non_empty_str(item.get("title"))?;
        let url = non_empty_str(item.get("url")).or_else(|| {
            non_empty_str(item.pointer("/externalIds/DOI"))
                .map(|doi| format!("https://doi.org/{doi}"))
        })?;
        let snippet = non_empty_str(item.get("abstract")).unwrap_or_else(|| {
            academic_snippet(
                non_empty_str(item.get("venue")),
                json_year(item.get("year")),
                None,
            )
        });
        Some(RawHit {
            title,
            url,
            snippet,
        })
    }

    fn json_year(value: Option<&Value>) -> Option<String> {
        value.and_then(Value::as_u64).map(|year| year.to_string())
    }

    fn non_empty_str(value: Option<&Value>) -> Option<String> {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
    }

    fn academic_snippet(
        venue: Option<String>,
        year: Option<String>,
        authors: Option<String>,
    ) -> String {
        [venue, year, authors]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{2014} ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn web_engines_table_names_are_unique_and_resolvable() {
        let names: BTreeSet<&str> = WEB_ENGINES.iter().map(|spec| spec.name).collect();
        assert_eq!(names.len(), WEB_ENGINES.len());
        for spec in WEB_ENGINES {
            assert_eq!(
                engine_by_name(spec.name).unwrap().id,
                spec.id,
                "{}",
                spec.name
            );
            assert_eq!(
                engine_by_id(spec.id).unwrap().name,
                spec.name,
                "{}",
                spec.name
            );
            if spec.url_from_config {
                assert!(spec.url.is_empty(), "{}", spec.name);
            } else {
                assert!(spec.url.starts_with("https://"), "{}", spec.name);
            }
        }
    }

    #[test]
    fn web_engines_table_fallbacks_resolve() {
        for spec in WEB_ENGINES {
            if spec.fallback.is_some() {
                assert!(fallback_engine(spec).is_some(), "{}", spec.name);
            }
        }
    }

    #[test]
    fn every_mode_has_default_engines_and_google_is_opt_in() {
        for mode in Mode::all() {
            let engines = engines_for_mode(mode, &WebSearchConfig::default());
            assert!(!engines.is_empty(), "{mode}");
            assert!(
                engines.iter().all(|spec| spec.id != WebEngineId::Google),
                "{mode}"
            );
        }
    }

    #[test]
    fn resolve_url_falls_back_to_the_table_and_needs_config_for_searxng() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            searxng_url: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "fixed engine ignores the config",
                engine: "bing",
                searxng_url: "https://searx.example.org",
                want: Some("https://www.bing.com/search"),
            },
            Case {
                name: "searxng builds the search path",
                engine: "searxng",
                searxng_url: "https://searx.example.org",
                want: Some("https://searx.example.org/search"),
            },
            Case {
                name: "searxng tolerates a trailing slash",
                engine: "searxng",
                searxng_url: "https://searx.example.org/",
                want: Some("https://searx.example.org/search"),
            },
            Case {
                name: "unconfigured searxng resolves to nothing",
                engine: "searxng",
                searxng_url: "",
                want: None,
            },
            Case {
                name: "blank searxng url resolves to nothing",
                engine: "searxng",
                searxng_url: "   ",
                want: None,
            },
        ];
        for case in cases {
            let config = WebSearchConfig {
                searxng_url: case.searxng_url.to_string(),
                ..WebSearchConfig::default()
            };
            let spec = engine_by_name(case.engine).unwrap();
            let got = resolve_url(spec, &config);
            assert_eq!(got.as_deref(), case.want, "{}", case.name);
        }
    }

    #[test]
    fn searxng_leads_the_defaults_only_once_configured() {
        let off = WebSearchConfig::default();
        let names: Vec<&str> = engines_for_mode(Mode::General, &off)
            .iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["ddg", "bing"]);

        let on = WebSearchConfig {
            searxng_url: "https://searx.example.org".to_string(),
            ..WebSearchConfig::default()
        };
        let names: Vec<&str> = engines_for_mode(Mode::General, &on)
            .iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["searxng", "ddg", "bing"]);
    }

    #[test]
    fn an_explicit_allowlist_still_wins_over_the_searxng_prepend() {
        let config = WebSearchConfig {
            searxng_url: "https://searx.example.org".to_string(),
            engines: vec!["mojeek".to_string()],
            ..WebSearchConfig::default()
        };
        let names: Vec<&str> = engines_for_mode(Mode::General, &config)
            .iter()
            .map(|spec| spec.name)
            .collect();
        assert_eq!(names, vec!["mojeek"]);
    }

    #[test]
    fn scientific_mode_prioritizes_academic_engines() {
        let engines = engines_for_mode(Mode::Scientific, &WebSearchConfig::default());
        assert_eq!(engines[0].category, WebCategory::Academic);
        assert!(engines.iter().any(|spec| spec.category == WebCategory::Web));
    }

    #[test]
    fn page_grounding_follows_mode_list_and_master_switch() {
        struct Case {
            name: &'static str,
            mode: Mode,
            config: WebSearchConfig,
            want: bool,
        }
        let cases = [
            Case {
                name: "scientific is grounded by default",
                mode: Mode::Scientific,
                config: WebSearchConfig::default(),
                want: true,
            },
            Case {
                name: "deep is grounded by default",
                mode: Mode::Deep,
                config: WebSearchConfig::default(),
                want: true,
            },
            Case {
                name: "general is not grounded by default",
                mode: Mode::General,
                config: WebSearchConfig::default(),
                want: false,
            },
            Case {
                name: "news is not grounded by default",
                mode: Mode::News,
                config: WebSearchConfig::default(),
                want: false,
            },
            Case {
                name: "explicit list overrides the defaults",
                mode: Mode::General,
                config: WebSearchConfig {
                    ground_modes: vec!["General".to_string()],
                    ..WebSearchConfig::default()
                },
                want: true,
            },
            Case {
                name: "empty list disables grounding",
                mode: Mode::Scientific,
                config: WebSearchConfig {
                    ground_modes: Vec::new(),
                    ..WebSearchConfig::default()
                },
                want: false,
            },
            Case {
                name: "disabled websearch wins over the list",
                mode: Mode::Scientific,
                config: WebSearchConfig::disabled(),
                want: false,
            },
        ];
        for case in cases {
            assert_eq!(
                page_grounding_enabled(case.mode, &case.config),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn engines_for_mode_honors_the_allowlist() {
        struct Case {
            name: &'static str,
            allowlist: &'static [&'static str],
            want: &'static [&'static str],
        }
        let cases = [
            Case {
                name: "explicit engines override mode defaults",
                allowlist: &["google", "bing"],
                want: &["google", "bing"],
            },
            Case {
                name: "unknown names are dropped",
                allowlist: &["altavista", "mojeek"],
                want: &["mojeek"],
            },
            Case {
                name: "empty allowlist keeps defaults",
                allowlist: &[],
                want: &["ddg", "bing"],
            },
        ];
        for case in cases {
            let config = WebSearchConfig {
                engines: case.allowlist.iter().map(ToString::to_string).collect(),
                ..WebSearchConfig::default()
            };
            let names: Vec<&str> = engines_for_mode(Mode::General, &config)
                .iter()
                .map(|spec| spec.name)
                .collect();
            assert_eq!(names, case.want, "{}", case.name);
        }
    }

    #[test]
    fn request_params_start_with_the_query_and_add_mailto_only_where_supported() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            mailto: &'static str,
            want_first: (&'static str, &'static str),
            want_mailto: bool,
        }
        let cases = [
            Case {
                name: "ddg ignores mailto",
                engine: "ddg",
                mailto: "user@example.com",
                want_first: ("q", "rust"),
                want_mailto: false,
            },
            Case {
                name: "openalex appends mailto",
                engine: "openalex",
                mailto: "user@example.com",
                want_first: ("search", "rust"),
                want_mailto: true,
            },
            Case {
                name: "crossref without mailto configured",
                engine: "crossref",
                mailto: "  ",
                want_first: ("query", "rust"),
                want_mailto: false,
            },
        ];
        for case in cases {
            let spec = engine_by_name(case.engine).unwrap();
            let params = request_params(spec, "rust", case.mailto);
            assert_eq!(
                params[0],
                (case.want_first.0.to_string(), case.want_first.1.to_string()),
                "{}",
                case.name
            );
            assert_eq!(
                params.iter().any(|(key, _)| key == "mailto"),
                case.want_mailto,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn ddg_redirects_decode_to_the_target_url() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "protocol relative redirect",
                input: "//duckduckgo.com/l/?uddg=https%3A%2F%2Fwww.rust%2Dlang.org%2F&rut=r1",
                want: Some("https://www.rust-lang.org/"),
            },
            Case {
                name: "absolute redirect",
                input: "https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%20b",
                want: Some("https://example.com/a b"),
            },
            Case {
                name: "direct url passes through",
                input: "https://example.com/page?x=1",
                want: Some("https://example.com/page?x=1"),
            },
            Case {
                name: "ad link without uddg is rejected",
                input: "//duckduckgo.com/y.js?ad_domain=example.com&rut=ad1",
                want: None,
            },
            Case {
                name: "relative garbage is rejected",
                input: "/html/?q=next",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                decode_ddg_redirect(case.input).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn bing_redirects_decode_the_base64url_target() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "wrapped click url",
                input: "https://www.bing.com/ck/a?!&&p=deadbeef&u=a1aHR0cHM6Ly93d3cucnVzdC1sYW5nLm9yZy8&ntb=1",
                want: Some("https://www.rust-lang.org/"),
            },
            Case {
                name: "direct url passes through",
                input: "https://en.wikipedia.org/wiki/Rust",
                want: Some("https://en.wikipedia.org/wiki/Rust"),
            },
            Case {
                name: "wrapped url with invalid base64 is rejected",
                input: "https://www.bing.com/ck/a?u=a1%%%invalid",
                want: None,
            },
            Case {
                name: "wrapped url without u param is rejected",
                input: "https://www.bing.com/ck/a?p=only",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                decode_bing_redirect(case.input).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn google_redirects_unwrap_the_q_param() {
        struct Case {
            name: &'static str,
            input: &'static str,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "wrapped result url",
                input: "/url?q=https%3A%2F%2Fexample.com%2Fdoc&sa=U",
                want: Some("https://example.com/doc"),
            },
            Case {
                name: "direct url passes through",
                input: "https://example.com/doc",
                want: Some("https://example.com/doc"),
            },
            Case {
                name: "internal path is rejected",
                input: "/search?q=more",
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                decode_google_redirect(case.input).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn encoded_params_percent_encodes_the_query() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            query: &'static str,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "spaces become plus signs",
                engine: "bing",
                query: "rust programming language",
                want: "q=rust+programming+language",
            },
            Case {
                name: "reserved characters are escaped",
                engine: "mojeek",
                query: "a&b=c",
                want: "q=a%26b%3Dc",
            },
        ];
        for case in cases {
            let spec = engine_by_name(case.engine).unwrap();
            assert_eq!(
                encoded_params(spec, case.query, ""),
                case.want,
                "{}",
                case.name
            );
        }
    }

    fn hit(title: &str, url: &str, snippet: &str) -> WebHit {
        WebHit {
            title: title.to_string(),
            url: url.to_string(),
            snippet: snippet.to_string(),
            engine: "ddg",
        }
    }

    #[test]
    fn hits_prompt_block_is_empty_without_hits() {
        assert_eq!(hits_prompt_block(&[]), "");
    }

    #[test]
    fn hits_prompt_block_numbers_leads_and_asks_for_verification() {
        let hits = [
            hit(
                "Rust",
                "https://www.rust-lang.org/",
                "A language empowering everyone.",
            ),
            hit("Wikipedia", "https://en.wikipedia.org/wiki/Rust", ""),
        ];
        let block = hits_prompt_block(&hits);
        assert!(block.starts_with("Candidate sources found by conventional search engines"));
        assert!(block.contains("1. Rust \u{2014} https://www.rust-lang.org/\n"));
        assert!(block.contains("   A language empowering everyone.\n"));
        assert!(block.contains("2. Wikipedia \u{2014} https://en.wikipedia.org/wiki/Rust\n"));
        assert!(block.contains("verify them before citing"));
        assert!(block.ends_with("beyond this list.\n"));
    }

    #[test]
    fn hits_as_findings_uses_snippet_or_title_and_drops_invalid_urls() {
        struct Case {
            name: &'static str,
            hit: WebHit,
            want_claim: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "snippet becomes the claim",
                hit: hit("Title", "https://example.com/a", "The snippet."),
                want_claim: Some("The snippet."),
            },
            Case {
                name: "empty snippet falls back to the title",
                hit: hit("Title", "https://example.com/b", ""),
                want_claim: Some("Title"),
            },
            Case {
                name: "invalid url is dropped",
                hit: hit("Title", "ftp://example.com", "snippet"),
                want_claim: None,
            },
        ];
        for case in cases {
            let findings = hits_as_findings(std::slice::from_ref(&case.hit), "en");
            assert_eq!(
                findings.first().map(|finding| finding.claim.as_str()),
                case.want_claim,
                "{}",
                case.name
            );
            if let Some(finding) = findings.first() {
                assert_eq!(finding.lang, "en", "{}", case.name);
                assert_eq!(finding.source_title, case.hit.title, "{}", case.name);
                assert!(finding.image_url.is_empty(), "{}", case.name);
            }
        }
    }

    #[test]
    fn snippet_sub_results_align_hits_with_their_sub_queries() {
        let sub_queries = [
            SubQuery {
                query: "first".to_string(),
                lang: "en".to_string(),
                rationale: String::new(),
            },
            SubQuery {
                query: "second".to_string(),
                lang: "pt-BR".to_string(),
                rationale: String::new(),
            },
        ];
        let hits = [
            vec![hit("A", "https://a.example/page", "snippet a")],
            Vec::new(),
        ];
        let results = snippet_sub_results(&sub_queries, &hits);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].query, "first");
        assert!(results[0].response.summary.is_empty());
        assert_eq!(results[0].response.findings[0].claim, "snippet a");
        assert_eq!(results[0].response.findings[0].lang, "en");
        assert!(results[1].response.findings.is_empty());
    }

    #[test]
    fn allowed_urls_with_hits_unions_findings_and_normalized_hit_urls() {
        let merged = MergedFindings {
            findings: hits_as_findings(&[hit("A", "https://a.example/page", "c")], "en"),
            summaries: vec![],
        };
        let hits = [
            hit("B", "https://B.Example/Path/", "s"),
            hit("Bad", "not a url", "s"),
        ];
        let allowed = allowed_urls_with_hits(&merged, &hits);
        assert_eq!(
            allowed,
            BTreeSet::from([
                "https://a.example/page".to_string(),
                "https://b.example/Path".to_string(),
            ])
        );
    }
}

#[cfg(all(test, feature = "websearch"))]
mod parse_tests {
    use super::*;

    const DDG_HTML: &str = include_str!("../../tests/fixtures/websearch/ddg_html.html");
    const DDG_LITE: &str = include_str!("../../tests/fixtures/websearch/ddg_lite.html");
    const BING: &str = include_str!("../../tests/fixtures/websearch/bing.html");
    const MOJEEK: &str = include_str!("../../tests/fixtures/websearch/mojeek.html");
    const GOOGLE: &str = include_str!("../../tests/fixtures/websearch/google.html");
    const OPENALEX: &str = include_str!("../../tests/fixtures/websearch/openalex.json");
    const CROSSREF: &str = include_str!("../../tests/fixtures/websearch/crossref.json");
    const SEMANTIC_SCHOLAR: &str =
        include_str!("../../tests/fixtures/websearch/semantic_scholar.json");
    const SEARXNG: &str = include_str!("../../tests/fixtures/websearch/searxng.json");

    fn hits(engine: &str, body: &str) -> Vec<WebHit> {
        parse_hits(engine_by_name(engine).unwrap(), body, 10)
    }

    #[test]
    fn every_parser_extracts_the_expected_first_hit() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            body: &'static str,
            want_count: usize,
            want_title: &'static str,
            want_url: &'static str,
            want_snippet: &'static str,
        }
        let cases = [
            Case {
                name: "ddg html skips ads and decodes redirects",
                engine: "ddg",
                body: DDG_HTML,
                want_count: 2,
                want_title: "Rust Programming Language",
                want_url: "https://www.rust-lang.org/",
                want_snippet: "A language empowering everyone to build reliable and efficient software.",
            },
            Case {
                name: "ddg lite pairs links with snippet rows",
                engine: "ddg-lite",
                body: DDG_LITE,
                want_count: 2,
                want_title: "Rust Programming Language",
                want_url: "https://www.rust-lang.org/",
                want_snippet: "A language empowering everyone to build reliable and efficient software.",
            },
            Case {
                name: "bing unwraps click tracking urls",
                engine: "bing",
                body: BING,
                want_count: 2,
                want_title: "Rust Programming Language",
                want_url: "https://www.rust-lang.org/",
                want_snippet: "A language empowering everyone to build reliable and efficient software.",
            },
            Case {
                name: "mojeek reads direct urls",
                engine: "mojeek",
                body: MOJEEK,
                want_count: 2,
                want_title: "Rust Programming Language",
                want_url: "https://www.rust-lang.org/",
                want_snippet: "A language empowering everyone to build reliable and efficient software.",
            },
            Case {
                name: "google keeps only results with headings",
                engine: "google",
                body: GOOGLE,
                want_count: 2,
                want_title: "Rust Programming Language",
                want_url: "https://www.rust-lang.org/",
                want_snippet: "A language empowering everyone to build reliable and efficient software.",
            },
            Case {
                name: "searxng reads the json api and tolerates empty content",
                engine: "searxng",
                body: SEARXNG,
                want_count: 3,
                want_title: "Tokio - An asynchronous Rust runtime",
                want_url: "https://tokio.rs/tokio/tutorial",
                want_snippet: "Tokio is an asynchronous runtime for the Rust programming language, providing the building blocks for writing network applications.",
            },
            Case {
                name: "openalex prefers doi and composes venue year authors",
                engine: "openalex",
                body: OPENALEX,
                want_count: 2,
                want_title: "Caffeine Effects on Sleep Taken 0, 3, or 6 Hours before Going to Bed",
                want_url: "https://doi.org/10.5664/jcsm.3170",
                want_snippet: "Journal of Clinical Sleep Medicine \u{2014} 2013 \u{2014} Christopher L. Drake, Timothy Roehrs",
            },
            Case {
                name: "crossref drops untitled records",
                engine: "crossref",
                body: CROSSREF,
                want_count: 1,
                want_title: "Caffeine and sleep",
                want_url: "https://doi.org/10.1053/smrv.1999.0088",
                want_snippet: "Sleep Medicine Reviews \u{2014} 2000 \u{2014} Ian Hindmarch",
            },
            Case {
                name: "semantic scholar uses the abstract as snippet",
                engine: "s2",
                body: SEMANTIC_SCHOLAR,
                want_count: 2,
                want_title: "Caffeine consumption and sleep quality in a large observational cohort",
                want_url: "https://www.semanticscholar.org/paper/649def34f8be52c8b66281af98ae884c09aef38b",
                want_snippet: "We examined the association between daily caffeine intake and objective sleep measures in 7,000 adults. Higher intake within six hours of bedtime was associated with reduced total sleep time and increased sleep onset latency.",
            },
        ];
        for case in cases {
            let parsed = hits(case.engine, case.body);
            assert_eq!(parsed.len(), case.want_count, "{}", case.name);
            assert_eq!(parsed[0].title, case.want_title, "{}", case.name);
            assert_eq!(parsed[0].url, case.want_url, "{}", case.name);
            assert_eq!(parsed[0].snippet, case.want_snippet, "{}", case.name);
            assert_eq!(
                parsed[0].engine,
                engine_by_name(case.engine).unwrap().name,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn semantic_scholar_falls_back_to_doi_and_year() {
        let parsed = hits("s2", SEMANTIC_SCHOLAR);
        assert_eq!(parsed[1].url, "https://doi.org/10.1000/stim.2016");
        assert_eq!(parsed[1].snippet, "2016");
    }

    #[test]
    fn parse_hits_caps_results_at_max() {
        let parsed = parse_hits(engine_by_name("ddg").unwrap(), DDG_HTML, 1);
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn parse_hits_deduplicates_by_normalized_url() {
        let body = DDG_LITE.replace(
            "uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FRust_(programming_language)",
            "uddg=https%3A%2F%2Fwww.rust%2Dlang.org",
        );
        let parsed = parse_hits(engine_by_name("ddg-lite").unwrap(), &body, 10);
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn parse_hits_survives_garbage_bodies() {
        struct Case {
            name: &'static str,
            engine: &'static str,
            body: &'static str,
        }
        let cases = [
            Case {
                name: "empty html",
                engine: "ddg",
                body: "",
            },
            Case {
                name: "captcha page",
                engine: "bing",
                body: "<html><body><h1>Verify you are human</h1></body></html>",
            },
            Case {
                name: "json error payload",
                engine: "openalex",
                body: "{\"error\":\"rate limited\"}",
            },
            Case {
                name: "not json at all",
                engine: "crossref",
                body: "<html>gateway timeout</html>",
            },
        ];
        for case in cases {
            let parsed = hits(case.engine, case.body);
            assert!(parsed.is_empty(), "{}", case.name);
        }
    }

    #[test]
    fn long_snippets_are_truncated_with_an_ellipsis() {
        let long_abstract = "word ".repeat(100);
        let body = format!(
            "{{\"data\":[{{\"title\":\"T\",\"url\":\"https://example.com\",\"abstract\":\"{}\"}}]}}",
            long_abstract.trim()
        );
        let parsed = parse_hits(engine_by_name("s2").unwrap(), &body, 10);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].snippet.chars().count(), SNIPPET_MAX_CHARS + 1);
        assert!(parsed[0].snippet.ends_with('\u{2026}'));
    }
}
