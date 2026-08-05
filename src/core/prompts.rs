use crate::core::answer::{ANSWER_SCHEMA, FAST_ANSWER_SCHEMA};
use crate::core::citations::MergedFindings;
use crate::core::context::{ResearchContext, context_prompt_block};
use crate::core::mode::ModeSpec;
use crate::core::plan::{SearchPlan, SubQuery};
use crate::core::readability::{PageText, pages_prompt_block};
use crate::core::websearch::{WebHit, hits_prompt_block};

pub const EXPANSION_MARKER: &str = "MUADDIB:EXPAND";
pub const SUB_SEARCH_MARKER: &str = "MUADDIB:SUBSEARCH";
pub const SYNTHESIS_MARKER: &str = "MUADDIB:SYNTH";
pub const FAST_MARKER: &str = "MUADDIB:FAST";
pub const REFLECTION_MARKER: &str = "MUADDIB:REFLECT";

pub fn reflection_prompt(plan: &SearchPlan, draft: &str, max_gaps: usize) -> String {
    format!(
        "[task {REFLECTION_MARKER}] You are the critic of a meta-search engine. A draft answer \
         has just been compiled and you decide whether it is good enough to ship.\n\
         Original query: {original}\n\
         Search mode: {label}. {instructions}\n\
         Sub-queries already searched, so do not repeat them:\n{searched}\n\
         Draft answer, as Markdown:\n{draft}\n\
         Name only the gaps that another web search could actually close: a claim with no \
         source behind it, a facet of the original query the draft never touches, a number or \
         date the draft leaves vague, or a position the draft asserts without looking for the \
         opposing evidence.\n\
         Rules:\n\
         - Return at most {max_gaps} gaps, ordered by how much they weaken the answer.\n\
         - Each gap must be a concrete, searchable query, not an instruction to the writer.\n\
         - Never repeat a sub-query already listed above, in any wording.\n\
         - Return an empty list when the draft is well supported. An empty list is the right \
         answer more often than not; do not invent a gap to look thorough.\n\
         Reply with ONLY this JSON, no prose:\n\
         {{\"gaps\":[{{\"query\":\"...\",\"lang\":\"BCP-47 tag\",\"rationale\":\"...\"}}]}}",
        original = plan.original,
        label = plan.mode.spec().label,
        instructions = plan.mode.spec().instructions,
        searched = searched_lines(&plan.sub_queries),
    )
}

fn searched_lines(sub_queries: &[SubQuery]) -> String {
    sub_queries
        .iter()
        .map(|sub| format!("- [{}] {}\n", sub.lang, sub.query))
        .collect()
}

pub fn fast_prompt(
    query: &str,
    mode: &ModeSpec,
    answer_lang: &str,
    inline_schema: bool,
    context: &ResearchContext,
) -> String {
    format!(
        "[task {FAST_MARKER}] You are the fast lane of a meta-search engine. Speed is the \
         priority: the whole answer must be produced in a few seconds.\n\
         Run ONE web search for the query below, consult at most 4 pages, and answer immediately.\n\
         Search mode: {label}. {instructions}\n\
         {context_block}\
         Write the entire answer in {answer_lang}. Every title, paragraph, list item, and \
         follow-up must be in {answer_lang}.\n\
         Query: {query}\n\
         Rules:\n\
         - Keep it short: at most two brief paragraphs, or one paragraph plus one list.\n\
         - Use only paragraph, list, and heading blocks. Never emit a chart, diagram, image, \
         quote, or table block.\n\
         - Every paragraph and list item must cite at least one source through its source_ids.\n\
         - Only report URLs of pages you actually consulted; never invent or guess a URL.\n\
         - Number sources starting at 1, at most 4 of them.\n\
         - Suggest up to 3 follow-up queries in the followups array.\n\
         - Do not run extra searches to broaden coverage; answer with what the first search \
         gives you.\n\
         {output_contract}",
        label = mode.label,
        instructions = mode.instructions,
        context_block = context_prompt_block(context),
        output_contract = fast_output_contract(inline_schema),
    )
}

fn fast_output_contract(inline_schema: bool) -> String {
    json_schema_or_tool_contract(inline_schema, FAST_ANSWER_SCHEMA)
}

fn structured_output_contract() -> String {
    "Return the answer by calling the StructuredOutput tool exactly once. \
     Never write the JSON as text first: writing it out and then calling the tool \
     generates the whole answer twice and doubles the wait."
        .to_string()
}

pub fn expansion_prompt(
    query: &str,
    mode: &ModeSpec,
    answer_lang: &str,
    breadth: u8,
    context: &ResearchContext,
) -> String {
    format!(
        "[task {EXPANSION_MARKER}] You are the query planner of a meta-search engine.\n\
         First rate the query complexity: \"simple\" when one direct search fully answers it \
         (a fact, a definition, a single lookup), \"standard\" otherwise.\n\
         For a simple query return exactly one sub-query: the query itself.\n\
         Otherwise expand the query below into at most {breadth} sub-queries covering distinct facets of the topic.{cross_language_rule}\n\
         Search mode: {label}. {instructions}\n\
         {context_block}\
         The final answer will be written in {answer_lang}.\n\
         Query: {query}\n\
         Reply with ONLY this JSON, no prose:\n\
         {{\"complexity\":\"simple|standard\",\"subqueries\":[{{\"query\":\"...\",\"lang\":\"BCP-47 tag\",\"rationale\":\"...\"}}]}}",
        cross_language_rule = cross_language_rule(mode),
        label = mode.label,
        instructions = mode.instructions,
        context_block = context_prompt_block(context),
    )
}

fn cross_language_rule(mode: &ModeSpec) -> &'static str {
    if mode.cross_language {
        "\nInclude at least one sub-query written in a different relevant language, \
         such as English or the language most associated with the topic."
    } else {
        ""
    }
}

pub fn sub_search_prompt(
    sub: &SubQuery,
    mode: &ModeSpec,
    hits: &[WebHit],
    pages: &[PageText],
) -> String {
    format!(
        "[task {SUB_SEARCH_MARKER}] Search the web for: {query}\n\
         Query language: {lang}.\n\
         Search mode: {label}. {instructions}\n\
         {hits_block}\
         {pages_block}\
         Collect concrete findings, each with the exact URL of the page that supports it.\n\
         Only report URLs of pages you actually consulted; never invent or guess a URL.\n\
         When a consulted page shows a relevant image (photo, chart, figure), add the \
         direct URL of that image file as image_url; omit image_url otherwise.\n\
         Reply with ONLY this JSON, no prose:\n\
         {{\"summary\":\"...\",\"findings\":[{{\"claim\":\"...\",\"source_title\":\"...\",\
         \"source_url\":\"https://...\",\"lang\":\"...\",\"image_url\":\"https://...\"}}]}}",
        query = sub.query,
        lang = sub.lang,
        label = mode.label,
        instructions = mode.instructions,
        hits_block = hits_prompt_block(hits),
        pages_block = pages_prompt_block(pages),
    )
}

pub fn synthesis_prompt(
    plan: &SearchPlan,
    merged: &MergedFindings,
    inline_schema: bool,
    context: &ResearchContext,
) -> String {
    format!(
        "[task {SYNTHESIS_MARKER}] You are the answer compiler of a meta-search engine.\n\
         Original query: {original}\n\
         Search mode: {label}. {instructions}\n\
         {context_block}\
         Write the entire answer in {answer_lang}. Every title, paragraph, label, and follow-up \
         must be in {answer_lang}.\n\
         Findings collected by parallel web searches, as JSON:\n{findings_json}\n\
         Compose one well-structured answer from these findings.\n\
         Rules:\n\
         - Every paragraph, list item, quote, table, and chart must cite at least one source \
         through its source_ids.\n\
         - The sources array may only contain URLs that appear in the findings above; never \
         invent a URL.\n\
         - Number sources starting at 1.\n\
         - Add a chart block when the findings contain comparable numbers.\n\
         - Add a diagram block that visualizes the answer's core structure: diagram_type \
         \"flow\" for processes or causal chains, \"timeline\" for chronologies; give each \
         item a short label and an optional one-line detail.\n\
         - Add an image block (url plus a short caption) when a finding carries an \
         image_url worth showing; use only image_url values from the findings, never \
         another URL.\n\
         - Add a conflict block ONLY when the findings genuinely disagree: two or more \
         sources making incompatible claims about the same fact. Give each position its \
         own claim and source_ids, and set kind to \"direct\" for a flat contradiction, \
         \"temporal\" when the sources describe different points in time, \"indirect\" when \
         they are inconsistent only taken together. Never manufacture a disagreement to \
         fill the block, and never use it to contrast a source with your own knowledge.\n\
         {source_notes_rule}\
         - Keep prose tight: prefer short paragraphs, lists, tables, charts, and diagrams \
         over long text.\n\
         - Optionally set emphasis to \"highlight\" on the single block that carries the key \
         takeaway.\n\
         - Suggest up to 3 follow-up queries in the followups array.\n\
         {output_contract}",
        original = plan.original,
        label = plan.mode.spec().label,
        instructions = plan.mode.spec().instructions,
        context_block = context_prompt_block(context),
        answer_lang = plan.answer_lang,
        findings_json = findings_json(merged),
        source_notes_rule = source_notes_rule(plan.mode.spec()),
        output_contract = output_contract(inline_schema),
    )
}

fn source_notes_rule(mode: &ModeSpec) -> String {
    if !mode.source_notes {
        return String::new();
    }
    "         - When a source is contested, dated, or partisan, set its note to one short \
     clause saying why a reader should weigh it carefully. Leave note empty otherwise.\n"
        .to_string()
}

fn findings_json(merged: &MergedFindings) -> String {
    serde_json::to_string_pretty(merged).unwrap_or_else(|_| "{}".to_string())
}

fn output_contract(inline_schema: bool) -> String {
    json_schema_or_tool_contract(inline_schema, ANSWER_SCHEMA)
}

fn json_schema_or_tool_contract(inline_schema: bool, schema: &str) -> String {
    if inline_schema {
        format!("Reply with ONLY a JSON object matching this JSON Schema, no prose:\n{schema}")
    } else {
        structured_output_contract()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::citations::Finding;
    use crate::core::mode::Mode;

    fn sample_plan() -> SearchPlan {
        SearchPlan {
            original: "solar energy in brazil".to_string(),
            mode: Mode::Scientific,
            answer_lang: "pt-BR".to_string(),
            sub_queries: vec![],
        }
    }

    fn sample_merged() -> MergedFindings {
        MergedFindings {
            findings: vec![Finding {
                claim: "installed capacity grew".to_string(),
                source_title: "Report".to_string(),
                source_url: "https://example.com/report".to_string(),
                lang: "en".to_string(),
                image_url: String::new(),
            }],
            summaries: vec!["[q] short summary".to_string()],
        }
    }

    #[test]
    fn every_prompt_carries_its_routing_marker() {
        let mode = Mode::General.spec();
        let sub = SubQuery {
            query: "q".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        struct Case {
            name: &'static str,
            prompt: String,
            marker: &'static str,
        }
        let cases = [
            Case {
                name: "expansion",
                prompt: expansion_prompt("q", mode, "en", 3, &ResearchContext::default()),
                marker: EXPANSION_MARKER,
            },
            Case {
                name: "sub search",
                prompt: sub_search_prompt(&sub, mode, &[], &[]),
                marker: SUB_SEARCH_MARKER,
            },
            Case {
                name: "synthesis",
                prompt: synthesis_prompt(
                    &sample_plan(),
                    &sample_merged(),
                    false,
                    &ResearchContext::default(),
                ),
                marker: SYNTHESIS_MARKER,
            },
            Case {
                name: "fast",
                prompt: fast_prompt("q", mode, "en", false, &ResearchContext::default()),
                marker: FAST_MARKER,
            },
            Case {
                name: "reflection",
                prompt: reflection_prompt(&sample_plan(), "# Draft", 3),
                marker: REFLECTION_MARKER,
            },
        ];
        for case in cases {
            assert!(case.prompt.contains(case.marker), "{}", case.name);
        }
    }

    #[test]
    fn reflection_prompt_lists_what_was_searched_and_bans_repeats() {
        let plan = SearchPlan {
            sub_queries: vec![
                SubQuery {
                    query: "solar capacity brazil".to_string(),
                    lang: "en".to_string(),
                    rationale: String::new(),
                },
                SubQuery {
                    query: "capacidade solar instalada".to_string(),
                    lang: "pt-BR".to_string(),
                    rationale: String::new(),
                },
            ],
            ..sample_plan()
        };
        let prompt = reflection_prompt(&plan, "# Draft\n\nA claim.", 3);
        assert!(prompt.contains("- [en] solar capacity brazil"));
        assert!(prompt.contains("- [pt-BR] capacidade solar instalada"));
        assert!(prompt.contains("Never repeat a sub-query already listed above"));
        assert!(prompt.contains("at most 3 gaps"));
        assert!(prompt.contains("A claim."));
    }

    #[test]
    fn reflection_prompt_makes_an_empty_answer_the_easy_one() {
        let prompt = reflection_prompt(&sample_plan(), "# Draft", 3);
        assert!(prompt.contains("Return an empty list when the draft is well supported"));
        assert!(prompt.contains("do not invent a gap to look thorough"));
        assert!(prompt.contains("\"gaps\""));
    }

    #[test]
    fn expansion_prompt_embeds_mode_breadth_and_language() {
        let mode = Mode::Scientific.spec();
        let prompt = expansion_prompt(
            "quantum computing",
            mode,
            "pt-BR",
            4,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("at most 4 sub-queries"));
        assert!(prompt.contains(mode.instructions));
        assert!(prompt.contains("pt-BR"));
        assert!(prompt.contains("quantum computing"));
        assert!(prompt.contains("different relevant language"));
    }

    #[test]
    fn sub_search_prompt_forbids_invented_urls() {
        let sub = SubQuery {
            query: "graphene batteries".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        let prompt = sub_search_prompt(&sub, Mode::General.spec(), &[], &[]);
        assert!(prompt.contains("graphene batteries"));
        assert!(prompt.contains("never invent or guess a URL"));
        assert!(prompt.contains("\"findings\""));
    }

    #[test]
    fn sub_search_prompt_without_hits_matches_the_ungrounded_text() {
        let sub = SubQuery {
            query: "q".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        let prompt = sub_search_prompt(&sub, Mode::General.spec(), &[], &[]);
        assert!(!prompt.contains("Candidate sources"));
        assert!(prompt.contains("Search mode: General."));
        assert!(prompt.contains(&format!(
            "{}\nCollect concrete findings",
            Mode::General.spec().instructions
        )));
    }

    #[test]
    fn sub_search_prompt_grounds_the_search_with_candidate_hits() {
        let sub = SubQuery {
            query: "rust language".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        let hits = [WebHit {
            title: "Rust".to_string(),
            url: "https://www.rust-lang.org/".to_string(),
            snippet: "A language empowering everyone.".to_string(),
            engine: "ddg",
            ..Default::default()
        }];
        let prompt = sub_search_prompt(&sub, Mode::General.spec(), &hits, &[]);
        assert!(prompt.contains(SUB_SEARCH_MARKER));
        assert!(prompt.contains("Candidate sources found by conventional search engines"));
        assert!(prompt.contains("https://www.rust-lang.org/"));
        assert!(prompt.contains("verify them before citing"));
        let block_at = prompt.find("Candidate sources").unwrap();
        let findings_at = prompt.find("Collect concrete findings").unwrap();
        assert!(block_at < findings_at);
    }

    #[test]
    fn context_block_appears_only_in_follow_up_prompts() {
        use crate::core::context::ContextStep;
        let mode = Mode::General.spec();
        let context = ResearchContext {
            steps: vec![ContextStep {
                query: "earlier question".to_string(),
                summary: "earlier digest".to_string(),
                source_urls: vec!["https://one.example/a".to_string()],
            }],
            omitted: 0,
        };
        struct Case {
            name: &'static str,
            fresh: String,
            follow_up: String,
        }
        let cases = [
            Case {
                name: "expansion",
                fresh: expansion_prompt("q", mode, "en", 3, &ResearchContext::default()),
                follow_up: expansion_prompt("q", mode, "en", 3, &context),
            },
            Case {
                name: "synthesis",
                fresh: synthesis_prompt(
                    &sample_plan(),
                    &sample_merged(),
                    false,
                    &ResearchContext::default(),
                ),
                follow_up: synthesis_prompt(&sample_plan(), &sample_merged(), false, &context),
            },
            Case {
                name: "fast",
                fresh: fast_prompt("q", mode, "en", false, &ResearchContext::default()),
                follow_up: fast_prompt("q", mode, "en", false, &context),
            },
        ];
        for case in cases {
            assert!(!case.fresh.contains("research thread"), "{}", case.name);
            assert!(case.follow_up.contains("research thread"), "{}", case.name);
            assert!(case.follow_up.contains("earlier question"), "{}", case.name);
            assert!(
                case.follow_up.contains("https://one.example/a"),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn sub_search_prompt_grounds_the_search_with_page_content() {
        let sub = SubQuery {
            query: "rust language".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        let hits = [WebHit {
            title: "Rust".to_string(),
            url: "https://www.rust-lang.org/".to_string(),
            snippet: "A language empowering everyone.".to_string(),
            engine: "ddg",
            ..Default::default()
        }];
        let pages = [PageText {
            url: "https://www.rust-lang.org/".to_string(),
            text: "Rust is blazingly fast and memory-efficient.".to_string(),
        }];
        let prompt = sub_search_prompt(&sub, Mode::General.spec(), &hits, &pages);
        assert!(prompt.contains("Fetched page content"));
        assert!(prompt.contains("Rust is blazingly fast and memory-efficient."));
        let hits_at = prompt.find("Candidate sources").unwrap();
        let pages_at = prompt.find("Fetched page content").unwrap();
        let findings_at = prompt.find("Collect concrete findings").unwrap();
        assert!(hits_at < pages_at);
        assert!(pages_at < findings_at);
    }

    #[test]
    fn synthesis_prompt_embeds_findings_language_and_rules() {
        let prompt = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            false,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("solar energy in brazil"));
        assert!(prompt.contains("pt-BR"));
        assert!(prompt.contains("https://example.com/report"));
        assert!(prompt.contains(
            "never \
         invent a URL"
        ));
        assert!(prompt.contains("chart block"));
    }

    #[test]
    fn synthesis_prompt_mentions_emphasis() {
        let prompt = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            false,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("emphasis"));
        assert!(prompt.contains("\"highlight\""));
    }

    #[test]
    fn synthesis_prompt_requests_a_diagram_and_compact_blocks() {
        let prompt = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            false,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("diagram"));
        assert!(prompt.contains("\"flow\""));
        assert!(prompt.contains("\"timeline\""));
        assert!(prompt.contains("short paragraphs"));
    }

    #[test]
    fn sub_search_prompt_asks_for_page_images() {
        let sub = SubQuery {
            query: "q".to_string(),
            lang: "en".to_string(),
            rationale: String::new(),
        };
        let prompt = sub_search_prompt(&sub, Mode::General.spec(), &[], &[]);
        assert!(prompt.contains("image_url"));
        assert!(prompt.contains("direct URL of that image"));
    }

    #[test]
    fn synthesis_prompt_allows_image_blocks_from_findings_only() {
        let prompt = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            false,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("image block"));
        assert!(prompt.contains("image_url"));
    }

    #[test]
    fn expansion_prompt_asks_for_a_complexity_rating() {
        let prompt = expansion_prompt(
            "q",
            Mode::General.spec(),
            "en",
            3,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("complexity"));
        assert!(prompt.contains("\"simple\""));
        assert!(prompt.contains("exactly one sub-query"));
    }

    #[test]
    fn synthesis_prompt_inlines_schema_only_when_requested() {
        let with_schema = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            true,
            &ResearchContext::default(),
        );
        let without_schema = synthesis_prompt(
            &sample_plan(),
            &sample_merged(),
            false,
            &ResearchContext::default(),
        );
        assert!(with_schema.contains("$schema"));
        assert!(!without_schema.contains("$schema"));
    }

    #[test]
    fn fast_prompt_asks_for_one_search_and_bans_rich_blocks() {
        let prompt = fast_prompt(
            "capital of peru",
            Mode::General.spec(),
            "pt-BR",
            false,
            &ResearchContext::default(),
        );
        assert!(prompt.contains("capital of peru"));
        assert!(prompt.contains("pt-BR"));
        assert!(prompt.contains("ONE web search"));
        assert!(prompt.contains("at most 4 pages"));
        assert!(prompt.contains("never invent or guess a URL"));
        for banned in ["chart", "diagram", "image", "quote", "table"] {
            assert!(prompt.contains(banned), "{banned}");
        }
        assert!(prompt.contains("Never emit a chart, diagram, image, quote, or table block."));
    }

    #[test]
    fn fast_prompt_inlines_the_fast_schema_only_when_requested() {
        let with_schema = fast_prompt(
            "q",
            Mode::General.spec(),
            "en",
            true,
            &ResearchContext::default(),
        );
        let without_schema = fast_prompt(
            "q",
            Mode::General.spec(),
            "en",
            false,
            &ResearchContext::default(),
        );
        assert!(with_schema.contains("$schema"));
        assert!(!with_schema.contains("\"const\": \"chart\""));
        assert!(!without_schema.contains("$schema"));
        assert!(
            with_schema.len()
                < synthesis_prompt(
                    &sample_plan(),
                    &sample_merged(),
                    true,
                    &ResearchContext::default()
                )
                .len()
        );
    }

    #[test]
    fn schema_capable_engines_are_told_to_call_the_tool_not_write_json() {
        struct Case {
            name: &'static str,
            prompt: String,
        }
        let cases = [
            Case {
                name: "synthesis",
                prompt: synthesis_prompt(
                    &sample_plan(),
                    &sample_merged(),
                    false,
                    &ResearchContext::default(),
                ),
            },
            Case {
                name: "fast",
                prompt: fast_prompt(
                    "q",
                    Mode::General.spec(),
                    "en",
                    false,
                    &ResearchContext::default(),
                ),
            },
        ];
        for case in cases {
            assert!(
                case.prompt
                    .contains("calling the StructuredOutput tool exactly once"),
                "{}",
                case.name
            );
            assert!(
                !case
                    .prompt
                    .contains("Reply with ONLY the JSON answer object"),
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn engines_without_schema_support_still_get_a_text_contract() {
        struct Case {
            name: &'static str,
            prompt: String,
        }
        let cases = [
            Case {
                name: "synthesis",
                prompt: synthesis_prompt(
                    &sample_plan(),
                    &sample_merged(),
                    true,
                    &ResearchContext::default(),
                ),
            },
            Case {
                name: "fast",
                prompt: fast_prompt(
                    "q",
                    Mode::General.spec(),
                    "en",
                    true,
                    &ResearchContext::default(),
                ),
            },
        ];
        for case in cases {
            assert!(
                case.prompt.contains("Reply with ONLY a JSON object"),
                "{}",
                case.name
            );
            assert!(!case.prompt.contains("StructuredOutput"), "{}", case.name);
        }
    }

    #[test]
    fn prompts_leave_no_unresolved_placeholders() {
        let placeholders = [
            "{query}",
            "{breadth}",
            "{answer_lang}",
            "{original}",
            "{label}",
            "{instructions}",
            "{output_contract}",
        ];
        let prompts = [
            expansion_prompt(
                "q",
                Mode::General.spec(),
                "en",
                3,
                &ResearchContext::default(),
            ),
            synthesis_prompt(
                &sample_plan(),
                &sample_merged(),
                true,
                &ResearchContext::default(),
            ),
            fast_prompt(
                "q",
                Mode::General.spec(),
                "en",
                true,
                &ResearchContext::default(),
            ),
        ];
        for prompt in &prompts {
            for placeholder in placeholders {
                assert!(!prompt.contains(placeholder), "{placeholder}");
            }
        }
    }
}
