use crate::core::answer::Answer;
use crate::core::plan::SearchPlan;
use crate::pipeline::{LinkStatus, SearchEvent, SearchHandle};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubQueryState {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pulse {
    pub source: usize,
    pub started: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageFetch {
    Ready(Vec<u8>),
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchOutcome {
    None,
    AnswerReady,
    Completed,
    Failed(String),
}

#[derive(Default)]
pub struct SearchState {
    pub plan: Option<SearchPlan>,
    pub web_hits: Option<usize>,
    pub progress: Vec<SubQueryState>,
    pub synthesizing: bool,
    pub started_at: Option<Instant>,
    pub handle: Option<SearchHandle>,
    pub answer: Option<Answer>,
    pub links: HashMap<u32, LinkStatus>,
    pub images: HashMap<String, ImageFetch>,
    pub reveal_started: Option<u64>,
    pub pulse: Option<Pulse>,
    pub failed_at: Option<u64>,
}

impl SearchState {
    pub fn begin(&mut self, now: Instant) {
        self.plan = None;
        self.web_hits = None;
        self.progress.clear();
        self.synthesizing = false;
        self.answer = None;
        self.links.clear();
        self.images.clear();
        self.reveal_started = None;
        self.pulse = None;
        self.failed_at = None;
        self.started_at = Some(now);
    }

    pub fn end(&mut self) {
        self.handle = None;
        self.synthesizing = false;
    }

    pub fn events_mut(&mut self) -> Option<&mut Receiver<SearchEvent>> {
        self.handle.as_mut().map(|handle| &mut handle.events)
    }

    pub fn elapsed_secs(&self, now: Instant) -> u64 {
        self.started_at.map_or(0, |started| {
            now.saturating_duration_since(started).as_secs()
        })
    }

    pub fn sub_query_state(&self, idx: usize) -> SubQueryState {
        self.progress
            .get(idx)
            .copied()
            .unwrap_or(SubQueryState::Pending)
    }

    pub fn apply_event(&mut self, event: SearchEvent, tick: u64) -> SearchOutcome {
        match event {
            SearchEvent::PlanReady(plan) => {
                self.progress = vec![SubQueryState::Pending; plan.sub_queries.len()];
                self.plan = Some(plan);
                SearchOutcome::None
            }
            SearchEvent::WebHits { count } => {
                self.web_hits = Some(count);
                SearchOutcome::None
            }
            SearchEvent::SubQueryStarted { idx } => {
                self.set_progress(idx, SubQueryState::Running);
                SearchOutcome::None
            }
            SearchEvent::SubQueryFinished { idx, ok } => {
                let state = if ok {
                    SubQueryState::Done
                } else {
                    SubQueryState::Failed
                };
                self.set_progress(idx, state);
                SearchOutcome::None
            }
            SearchEvent::SynthesisStarted => {
                self.synthesizing = true;
                SearchOutcome::None
            }
            SearchEvent::AnswerReady(answer) => {
                self.answer = Some(*answer);
                self.synthesizing = false;
                self.reveal_started = Some(tick);
                SearchOutcome::AnswerReady
            }
            SearchEvent::LinkChecked { source_id, status } => {
                self.links.insert(source_id, status);
                SearchOutcome::None
            }
            SearchEvent::ImageFetched { url, bytes } => {
                self.images
                    .insert(url, bytes.map_or(ImageFetch::Failed, ImageFetch::Ready));
                SearchOutcome::None
            }
            SearchEvent::Completed => {
                self.synthesizing = false;
                SearchOutcome::Completed
            }
            SearchEvent::Failed(message) => {
                self.synthesizing = false;
                self.failed_at = Some(tick);
                SearchOutcome::Failed(message)
            }
        }
    }

    fn set_progress(&mut self, idx: usize, state: SubQueryState) {
        if let Some(slot) = self.progress.get_mut(idx) {
            *slot = state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mode::Mode;
    use crate::core::plan::SubQuery;

    fn two_sub_query_plan() -> SearchPlan {
        SearchPlan {
            original: "q".to_string(),
            mode: Mode::General,
            answer_lang: "en".to_string(),
            sub_queries: vec![
                SubQuery {
                    query: "a".to_string(),
                    lang: "en".to_string(),
                    rationale: String::new(),
                },
                SubQuery {
                    query: "b".to_string(),
                    lang: "es".to_string(),
                    rationale: String::new(),
                },
            ],
        }
    }

    #[test]
    fn begin_clears_prior_results_and_stamps_the_start_time() {
        let mut state = SearchState {
            plan: Some(two_sub_query_plan()),
            progress: vec![SubQueryState::Done],
            synthesizing: true,
            answer: Some(Answer::default()),
            reveal_started: Some(3),
            pulse: Some(Pulse {
                source: 0,
                started: 1,
            }),
            failed_at: Some(9),
            ..SearchState::default()
        };
        state.links.insert(1, LinkStatus::Valid);
        state.images.insert("u".to_string(), ImageFetch::Failed);
        let now = Instant::now();
        state.begin(now);
        assert!(state.plan.is_none());
        assert!(state.progress.is_empty());
        assert!(!state.synthesizing);
        assert!(state.answer.is_none());
        assert!(state.links.is_empty());
        assert!(state.images.is_empty());
        assert!(state.reveal_started.is_none());
        assert!(state.pulse.is_none());
        assert!(state.failed_at.is_none());
        assert_eq!(state.started_at, Some(now));
    }

    #[tokio::test]
    async fn end_clears_the_handle_and_synthesizing_flag() {
        let mut state = SearchState::default();
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        state.handle = Some(SearchHandle::new(rx, tokio::spawn(async {})));
        state.synthesizing = true;
        state.end();
        assert!(state.handle.is_none());
        assert!(!state.synthesizing);
    }

    #[test]
    fn sub_query_state_defaults_to_pending_beyond_tracked_indices() {
        let state = SearchState {
            progress: vec![SubQueryState::Running],
            ..SearchState::default()
        };
        assert_eq!(state.sub_query_state(0), SubQueryState::Running);
        assert_eq!(state.sub_query_state(5), SubQueryState::Pending);
    }

    #[test]
    fn elapsed_secs_is_zero_without_a_start_time() {
        assert_eq!(SearchState::default().elapsed_secs(Instant::now()), 0);
    }

    #[test]
    fn apply_event_seeds_pending_progress_for_every_sub_query() {
        let mut state = SearchState::default();
        let outcome = state.apply_event(SearchEvent::PlanReady(two_sub_query_plan()), 0);
        assert_eq!(outcome, SearchOutcome::None);
        assert_eq!(state.progress, vec![SubQueryState::Pending; 2]);
        assert!(state.plan.is_some());
    }

    #[test]
    fn apply_event_stores_the_web_hit_count() {
        let mut state = SearchState::default();
        let outcome = state.apply_event(SearchEvent::WebHits { count: 7 }, 0);
        assert_eq!(outcome, SearchOutcome::None);
        assert_eq!(state.web_hits, Some(7));
        state.begin(Instant::now());
        assert_eq!(state.web_hits, None);
    }

    #[test]
    fn apply_event_tracks_sub_query_transitions() {
        let mut state = SearchState::default();
        state.apply_event(SearchEvent::PlanReady(two_sub_query_plan()), 0);
        state.apply_event(SearchEvent::SubQueryStarted { idx: 0 }, 0);
        assert_eq!(state.progress[0], SubQueryState::Running);
        state.apply_event(SearchEvent::SubQueryFinished { idx: 0, ok: true }, 0);
        assert_eq!(state.progress[0], SubQueryState::Done);
        state.apply_event(SearchEvent::SubQueryFinished { idx: 1, ok: false }, 0);
        assert_eq!(state.progress[1], SubQueryState::Failed);
    }

    #[test]
    fn apply_event_marks_the_answer_ready_and_stamps_the_reveal_tick() {
        let mut state = SearchState {
            synthesizing: true,
            ..SearchState::default()
        };
        let outcome = state.apply_event(SearchEvent::AnswerReady(Box::default()), 7);
        assert_eq!(outcome, SearchOutcome::AnswerReady);
        assert!(state.answer.is_some());
        assert!(!state.synthesizing);
        assert_eq!(state.reveal_started, Some(7));
    }

    #[test]
    fn apply_event_records_link_and_image_results() {
        let mut state = SearchState::default();
        state.apply_event(
            SearchEvent::LinkChecked {
                source_id: 1,
                status: LinkStatus::Valid,
            },
            0,
        );
        state.apply_event(
            SearchEvent::ImageFetched {
                url: "https://img".to_string(),
                bytes: Some(vec![1]),
            },
            0,
        );
        assert_eq!(state.links.get(&1), Some(&LinkStatus::Valid));
        assert_eq!(
            state.images.get("https://img"),
            Some(&ImageFetch::Ready(vec![1]))
        );
    }

    #[test]
    fn apply_event_surfaces_completion_and_failure_outcomes() {
        let mut state = SearchState {
            synthesizing: true,
            ..SearchState::default()
        };
        assert_eq!(
            state.apply_event(SearchEvent::Completed, 0),
            SearchOutcome::Completed
        );
        assert!(!state.synthesizing);
        state.synthesizing = true;
        assert_eq!(
            state.apply_event(SearchEvent::Failed("boom".to_string()), 6),
            SearchOutcome::Failed("boom".to_string())
        );
        assert!(!state.synthesizing);
        assert_eq!(state.failed_at, Some(6));
    }
}
