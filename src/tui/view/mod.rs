pub mod config_modal;
pub mod doc;
pub mod help;
pub mod home;
pub mod results;
pub mod searching;

use crate::tui::app::{App, Overlay, Screen};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &App) {
    match app.screen {
        Screen::Home => home::draw(frame, app),
        Screen::Searching => searching::draw(frame, app),
        Screen::Results => results::draw(frame, app),
    }
    match &app.overlay {
        Some(Overlay::Help) => help::draw(frame),
        Some(Overlay::Config(form)) => config_modal::draw(frame, app, form),
        None => {}
    }
}
