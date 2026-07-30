use crate::pipeline::SearchEvent;
use crossterm::event::KeyEvent;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize,
    Search(SearchEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    StartSearch { query: String },
    CancelSearch,
    OpenUrl(String),
    SaveConfig,
    Quit,
}
