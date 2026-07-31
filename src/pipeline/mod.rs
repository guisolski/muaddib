#[cfg(feature = "link-validation")]
pub mod images;
pub mod search;
#[cfg(feature = "link-validation")]
pub mod validate;

use crate::core::answer::Answer;
pub use crate::core::citations::LinkStatus;
use crate::core::plan::SearchPlan;

#[derive(Debug, Clone, PartialEq)]
pub enum SearchEvent {
    PlanReady(SearchPlan),
    SubQueryStarted { idx: usize },
    SubQueryFinished { idx: usize, ok: bool },
    SynthesisStarted,
    AnswerReady(Box<Answer>),
    LinkChecked { source_id: u32, status: LinkStatus },
    ImageFetched { url: String, bytes: Option<Vec<u8>> },
    Completed,
    Failed(String),
}

pub struct SearchHandle {
    pub events: tokio::sync::mpsc::Receiver<SearchEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl SearchHandle {
    pub fn new(
        events: tokio::sync::mpsc::Receiver<SearchEvent>,
        task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self { events, task }
    }

    pub fn abort(&self) {
        self.task.abort();
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}
