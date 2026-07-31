use crate::core::mode::Mode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const MAX_HISTORY: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryEntry {
    pub query: String,
    pub mode: Mode,
    pub fast: bool,
    pub at: u64,
}

impl Default for HistoryEntry {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: Mode::General,
            fast: false,
            at: 0,
        }
    }
}

pub fn history_path(home: &Path, xdg_state_home: Option<&Path>) -> PathBuf {
    xdg_state_home
        .map_or_else(|| home.join(".local").join("state"), Path::to_path_buf)
        .join("muaddib")
        .join("history.jsonl")
}

pub fn entry_line(entry: &HistoryEntry) -> String {
    serde_json::to_string(entry).unwrap_or_default()
}

pub fn parse_history(text: &str) -> Vec<HistoryEntry> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<HistoryEntry>(line).ok())
        .filter(|entry| !entry.query.trim().is_empty())
        .collect()
}

pub fn to_jsonl(entries: &[HistoryEntry]) -> String {
    entries
        .iter()
        .map(|entry| format!("{}\n", entry_line(entry)))
        .collect()
}

pub fn cap_entries(mut entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    if entries.len() > MAX_HISTORY {
        entries.drain(..entries.len() - MAX_HISTORY);
    }
    entries
}

pub fn recall_list(entries: &[HistoryEntry]) -> Vec<String> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut recall = Vec::new();
    for entry in entries.iter().rev() {
        if seen.insert(entry.query.as_str()) {
            recall.push(entry.query.clone());
        }
    }
    recall
}

pub fn repeats_latest(recall: &[String], query: &str) -> bool {
    recall.first().is_some_and(|latest| latest == query)
}

pub fn push_recall(recall: &mut Vec<String>, query: &str) {
    recall.retain(|entry| entry != query);
    recall.insert(0, query.to_string());
    recall.truncate(MAX_HISTORY);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(query: &str) -> HistoryEntry {
        HistoryEntry {
            query: query.to_string(),
            ..HistoryEntry::default()
        }
    }

    #[test]
    fn history_path_prefers_xdg_state_home() {
        struct Case {
            name: &'static str,
            home: &'static str,
            xdg: Option<&'static str>,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "xdg state home wins",
                home: "/home/user",
                xdg: Some("/custom/state"),
                want: "/custom/state/muaddib/history.jsonl",
            },
            Case {
                name: "falls back to local state under home",
                home: "/home/user",
                xdg: None,
                want: "/home/user/.local/state/muaddib/history.jsonl",
            },
        ];
        for case in cases {
            let got = history_path(Path::new(case.home), case.xdg.map(Path::new));
            assert_eq!(got, PathBuf::from(case.want), "{}", case.name);
        }
    }

    #[test]
    fn parse_history_skips_unusable_lines() {
        struct Case {
            name: &'static str,
            text: &'static str,
            want: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "well formed lines",
                text: "{\"query\":\"one\"}\n{\"query\":\"two\"}\n",
                want: vec!["one", "two"],
            },
            Case {
                name: "malformed line is skipped",
                text: "{\"query\":\"one\"}\nnot json at all\n{\"query\":\"two\"}\n",
                want: vec!["one", "two"],
            },
            Case {
                name: "unknown mode is skipped",
                text: "{\"query\":\"one\",\"mode\":\"nonsense\"}\n{\"query\":\"two\"}\n",
                want: vec!["two"],
            },
            Case {
                name: "blank query is skipped",
                text: "{\"query\":\"   \"}\n{\"query\":\"two\"}\n",
                want: vec!["two"],
            },
            Case {
                name: "empty file",
                text: "",
                want: vec![],
            },
        ];
        for case in cases {
            let got: Vec<String> = parse_history(case.text)
                .into_iter()
                .map(|entry| entry.query)
                .collect();
            assert_eq!(got, case.want, "{}", case.name);
        }
    }

    #[test]
    fn entries_round_trip_through_jsonl() {
        let entries = vec![
            HistoryEntry {
                query: "rust async runtimes".to_string(),
                mode: Mode::Deep,
                fast: false,
                at: 1_753_876_543,
            },
            HistoryEntry {
                query: "capital of peru".to_string(),
                mode: Mode::General,
                fast: true,
                at: 1_753_876_600,
            },
        ];
        assert_eq!(parse_history(&to_jsonl(&entries)), entries);
    }

    #[test]
    fn cap_entries_keeps_the_newest_window() {
        let entries: Vec<HistoryEntry> = (0..MAX_HISTORY + 10)
            .map(|index| entry(&format!("query {index}")))
            .collect();
        let capped = cap_entries(entries);
        assert_eq!(capped.len(), MAX_HISTORY);
        assert_eq!(capped[0].query, "query 10");
        assert_eq!(
            capped[MAX_HISTORY - 1].query,
            format!("query {}", MAX_HISTORY + 9)
        );
    }

    #[test]
    fn recall_list_is_newest_first_and_globally_deduped() {
        let entries = vec![
            entry("alpha"),
            entry("beta"),
            entry("alpha"),
            entry("gamma"),
        ];
        assert_eq!(recall_list(&entries), vec!["gamma", "alpha", "beta"]);
    }

    #[test]
    fn repeats_latest_only_matches_the_newest_query() {
        struct Case {
            name: &'static str,
            recall: Vec<&'static str>,
            query: &'static str,
            want: bool,
        }
        let cases = [
            Case {
                name: "same as newest",
                recall: vec!["alpha", "beta"],
                query: "alpha",
                want: true,
            },
            Case {
                name: "present but older",
                recall: vec!["alpha", "beta"],
                query: "beta",
                want: false,
            },
            Case {
                name: "absent",
                recall: vec!["alpha"],
                query: "gamma",
                want: false,
            },
            Case {
                name: "empty recall",
                recall: vec![],
                query: "alpha",
                want: false,
            },
        ];
        for case in cases {
            let recall: Vec<String> = case.recall.iter().map(ToString::to_string).collect();
            assert_eq!(
                repeats_latest(&recall, case.query),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn push_recall_moves_repeats_to_the_front() {
        let mut recall: Vec<String> = ["beta", "alpha"].iter().map(ToString::to_string).collect();
        push_recall(&mut recall, "alpha");
        assert_eq!(recall, vec!["alpha", "beta"]);
        push_recall(&mut recall, "gamma");
        assert_eq!(recall, vec!["gamma", "alpha", "beta"]);
    }

    #[test]
    fn push_recall_respects_the_cap() {
        let mut recall: Vec<String> = (0..MAX_HISTORY)
            .map(|index| format!("query {index}"))
            .collect();
        push_recall(&mut recall, "newest");
        assert_eq!(recall.len(), MAX_HISTORY);
        assert_eq!(recall[0], "newest");
    }
}
