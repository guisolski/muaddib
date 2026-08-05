use crate::pipeline::SearchEvent;
use crossterm::event::KeyEvent;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize { width: u16, height: u16 },
    Search(SearchEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    StartSearch { query: String },
    CancelSearch,
    OpenUrl(String),
    SaveConfig,
    SaveSession,
    CopyAnswer(String),
    ExportAnswer { filename: String, contents: String },
    ClearHistory,
    Quit,
}
