pub mod config_modal;
pub mod doc;
pub mod help;
pub mod home;
pub mod results;
pub mod searching;

use crate::tui::app::{App, Overlay, Screen};
use ratatui::Frame;

pub fn draw(frame: &mut Frame, app: &mut App) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::engines::{ENGINES, EngineStatus};
    use crate::tui::app::ConfigForm;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Position;
    use std::path::PathBuf;

    fn app() -> App {
        let statuses: Vec<EngineStatus> = ENGINES
            .iter()
            .map(|spec| EngineStatus {
                spec,
                available: true,
                path: Some(PathBuf::from("/fake/bin")),
            })
            .collect();
        let mut app = App::new(Config::default(), statuses, None, false);
        app.input = tui_input::Input::new("why is the sky blue?".to_string());
        app
    }

    fn render(app: &mut App, width: u16, height: u16) -> TestBackend {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().clone()
    }

    #[test]
    fn overlays_never_leave_the_search_cursor_lit() {
        struct Case {
            name: &'static str,
            overlay: Option<Overlay>,
            want_visible: bool,
        }
        let cases = [
            Case {
                name: "home shows the cursor",
                overlay: None,
                want_visible: true,
            },
            Case {
                name: "help overlay hides it",
                overlay: Some(Overlay::Help),
                want_visible: false,
            },
            Case {
                name: "config overlay hides it",
                overlay: Some(Overlay::Config(ConfigForm::from_state(
                    &Config::default(),
                    &[],
                ))),
                want_visible: false,
            },
        ];
        for case in cases {
            let mut app = app();
            app.overlay = case.overlay;
            let backend = render(&mut app, 80, 24);
            assert_eq!(backend.cursor_visible(), case.want_visible, "{}", case.name);
        }
    }

    #[test]
    fn the_search_cursor_stays_inside_the_input_box() {
        struct Case {
            name: &'static str,
            width: u16,
            height: u16,
        }
        let cases = [
            Case {
                name: "roomy terminal",
                width: 100,
                height: 30,
            },
            Case {
                name: "exact fit",
                width: 80,
                height: 10,
            },
            Case {
                name: "too short for the layout",
                width: 80,
                height: 6,
            },
            Case {
                name: "very short",
                width: 80,
                height: 3,
            },
            Case {
                name: "narrow",
                width: 14,
                height: 24,
            },
        ];
        for case in cases {
            let mut app = app();
            let backend = render(&mut app, case.width, case.height);
            if !backend.cursor_visible() {
                continue;
            }
            let Position { x, y } = backend.cursor_position();
            let cell = &backend.buffer()[(x, y)];
            assert!(
                !"╭╮╰╯─│".contains(cell.symbol()),
                "{} put the cursor on a border glyph {:?}",
                case.name,
                cell.symbol()
            );
            assert!(x < case.width && y < case.height, "{}", case.name);
        }
    }

    #[test]
    fn the_cursor_follows_the_text_and_never_reaches_the_right_border() {
        let mut app = app();
        app.input = tui_input::Input::new("x".repeat(200));
        let backend = render(&mut app, 80, 24);
        assert!(backend.cursor_visible());
        let Position { x, y } = backend.cursor_position();
        let row: String = (0..80u16)
            .map(|column| backend.buffer()[(column, y)].symbol())
            .collect();
        let right_border = row.rfind('╮').or_else(|| row.rfind('│'));
        assert!(right_border.is_some());
        assert!(
            usize::from(x) < right_border.unwrap(),
            "cursor at {x} reached the right border"
        );
        assert_eq!(
            backend.buffer()[(x, y)].symbol(),
            " ",
            "cursor sits on top of a character instead of after it"
        );
    }
}
