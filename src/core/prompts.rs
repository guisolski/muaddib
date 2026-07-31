use crate::core::answer::{ANSWER_SCHEMA, FAST_ANSWER_SCHEMA};
use crate::core::citations::MergedFindings;
use crate::core::mode::ModeSpec;
use crate::core::plan::{SearchPlan, SubQuery};

pub const EXPANSION_MARKER: &str = "FARO:EXPAND";
pub const SUB_SEARCH_MARKER: &str = "FARO:SUBSEARCH";
pub const SYNTHESIS_MARKER: &str = "FARO:SYNTH";
pub const FAST_MARKER: &str = "FARO:FAST";

pub fn fast_prompt(query: &str, mode: &ModeSpec, answer_lang: &str, inline_schema: bool) -> String {
    format!(
        "[task {FAST_MARKER}] You are the fast lane of a meta-search engine. Speed is the \
         priority: the whole answer must be produced in a few seconds.\n\
         Run ONE web search for the query below, consult at most 4 pages, and answer immediately.\n\
         Search mode: {label}. {instructions}\n\
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

pub fn expansion_prompt(query: &str, mode: &ModeSpec, answer_lang: &str, breadth: u8) -> String {
    format!(
        "[task {EXPANSION_MARKER}] You are the query planner of a meta-search engine.\n\
         First rate the query complexity: \"simple\" when one direct search fully answers it \
         (a fact, a definition, a single lookup), \"standard\" otherwise.\n\
         For a simple query return exactly one sub-query: the query itself.\n\
         Otherwise expand the query below into at most {breadth} sub-queries covering distinct facets of the topic.{cross_language_rule}\n\
         Search mode: {label}. {instructions}\n\
         The final answer will be written in {answer_lang}.\n\
         Query: {query}\n\
         Reply with ONLY this JSON, no prose:\n\
         {{\"complexity\":\"simple|standard\",\"subqueries\":[{{\"query\":\"...\",\"lang\":\"BCP-47 tag\",\"rationale\":\"...\"}}]}}",
        cross_language_rule = cross_language_rule(mode),
        label = mode.label,
        instructions = mode.instructions,
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

pub fn sub_search_prompt(sub: &SubQuery, mode: &ModeSpec) -> String {
    format!(
        "[task {SUB_SEARCH_MARKER}] Search the web for: {query}\n\
         Query language: {lang}.\n\
         Search mode: {label}. {instructions}\n\
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
    )
}

pub fn synthesis_prompt(plan: &SearchPlan, merged: &MergedFindings, inline_schema: bool) -> String {
    format!(
        "[task {SYNTHESIS_MARKER}] You are the answer compiler of a meta-search engine.\n\
         Original query: {original}\n\
         Search mode: {label}. {instructions}\n\
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
         - Keep prose tight: prefer short paragraphs, lists, tables, charts, and diagrams \
         over long text.\n\
         - Optionally set emphasis to \"highlight\" on the single block that carries the key \
         takeaway.\n\
         - Suggest up to 3 follow-up queries in the followups array.\n\
         {output_contract}",
        original = plan.original,
        label = plan.mode.spec().label,
        instructions = plan.mode.spec().instructions,
        answer_lang = plan.answer_lang,
        findings_json = findings_json(merged),
        output_contract = output_contract(inline_schema),
    )
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
                prompt: expansion_prompt("q", mode, "en", 3),
                marker: EXPANSION_MARKER,
            },
            Case {
                name: "sub search",
                prompt: sub_search_prompt(&sub, mode),
                marker: SUB_SEARCH_MARKER,
            },
            Case {
                name: "synthesis",
                prompt: synthesis_prompt(&sample_plan(), &sample_merged(), false),
                marker: SYNTHESIS_MARKER,
            },
            Case {
                name: "fast",
                prompt: fast_prompt("q", mode, "en", false),
                marker: FAST_MARKER,
            },
        ];
        for case in cases {
            assert!(case.prompt.contains(case.marker), "{}", case.name);
        }
    }

    #[test]
    fn expansion_prompt_embeds_mode_breadth_and_language() {
        let mode = Mode::Scientific.spec();
        let prompt = expansion_prompt("quantum computing", mode, "pt-BR", 4);
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
        let prompt = sub_search_prompt(&sub, Mode::General.spec());
        assert!(prompt.contains("graphene batteries"));
        assert!(prompt.contains("never invent or guess a URL"));
        assert!(prompt.contains("\"findings\""));
    }

    #[test]
    fn synthesis_prompt_embeds_findings_language_and_rules() {
        let prompt = synthesis_prompt(&sample_plan(), &sample_merged(), false);
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
        let prompt = synthesis_prompt(&sample_plan(), &sample_merged(), false);
        assert!(prompt.contains("emphasis"));
        assert!(prompt.contains("\"highlight\""));
    }

    #[test]
    fn synthesis_prompt_requests_a_diagram_and_compact_blocks() {
        let prompt = synthesis_prompt(&sample_plan(), &sample_merged(), false);
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
        let prompt = sub_search_prompt(&sub, Mode::General.spec());
        assert!(prompt.contains("image_url"));
        assert!(prompt.contains("direct URL of that image"));
    }

    #[test]
    fn synthesis_prompt_allows_image_blocks_from_findings_only() {
        let prompt = synthesis_prompt(&sample_plan(), &sample_merged(), false);
        assert!(prompt.contains("image block"));
        assert!(prompt.contains("image_url"));
    }

    #[test]
    fn expansion_prompt_asks_for_a_complexity_rating() {
        let prompt = expansion_prompt("q", Mode::General.spec(), "en", 3);
        assert!(prompt.contains("complexity"));
        assert!(prompt.contains("\"simple\""));
        assert!(prompt.contains("exactly one sub-query"));
    }

    #[test]
    fn synthesis_prompt_inlines_schema_only_when_requested() {
        let with_schema = synthesis_prompt(&sample_plan(), &sample_merged(), true);
        let without_schema = synthesis_prompt(&sample_plan(), &sample_merged(), false);
        assert!(with_schema.contains("$schema"));
        assert!(!without_schema.contains("$schema"));
    }

    #[test]
    fn fast_prompt_asks_for_one_search_and_bans_rich_blocks() {
        let prompt = fast_prompt("capital of peru", Mode::General.spec(), "pt-BR", false);
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
        let with_schema = fast_prompt("q", Mode::General.spec(), "en", true);
        let without_schema = fast_prompt("q", Mode::General.spec(), "en", false);
        assert!(with_schema.contains("$schema"));
        assert!(!with_schema.contains("\"const\": \"chart\""));
        assert!(!without_schema.contains("$schema"));
        assert!(with_schema.len() < synthesis_prompt(&sample_plan(), &sample_merged(), true).len());
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
                prompt: synthesis_prompt(&sample_plan(), &sample_merged(), false),
            },
            Case {
                name: "fast",
                prompt: fast_prompt("q", Mode::General.spec(), "en", false),
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
                prompt: synthesis_prompt(&sample_plan(), &sample_merged(), true),
            },
            Case {
                name: "fast",
                prompt: fast_prompt("q", Mode::General.spec(), "en", true),
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
            expansion_prompt("q", Mode::General.spec(), "en", 3),
            synthesis_prompt(&sample_plan(), &sample_merged(), true),
            fast_prompt("q", Mode::General.spec(), "en", true),
        ];
        for prompt in &prompts {
            for placeholder in placeholders {
                assert!(!prompt.contains(placeholder), "{placeholder}");
            }
        }
    }
}
