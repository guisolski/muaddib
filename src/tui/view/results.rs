use crate::tui::anim;
use crate::tui::app::{App, Focus};
use crate::tui::theme;
use crate::tui::view::doc::{self, DocSelection};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub fn draw(frame: &mut Frame, app: &App) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let Some(answer) = &app.answer else {
        frame.render_widget(
            Paragraph::new(Line::styled("no answer to show", theme::dim())).centered(),
            content,
        );
        return;
    };
    let width = doc::content_width(content.width);
    let selection = match app.focus {
        Focus::Body => DocSelection::None,
        Focus::Sources(index) => DocSelection::Source(index),
        Focus::Followups(index) => DocSelection::Followup(index),
    };
    let block_anim = anim::doc_anim(
        answer,
        app.reveal_started,
        app.pulse,
        app.tick,
        app.config.animations,
    );
    let rendered = doc::render_doc(answer, width, &app.links, selection, &block_anim);
    let [padded] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(content);
    frame.render_widget(
        Paragraph::new(rendered.lines).scroll((app.scroll, 0)),
        padded,
    );
    frame.render_widget(
        Paragraph::new(Line::styled(footer_hint(app.focus), theme::dim())).centered(),
        footer,
    );
}

fn footer_hint(focus: Focus) -> &'static str {
    match focus {
        Focus::Body => "j/k scroll · Tab focus · 1-9 source · n new · / refine · Esc home · q quit",
        Focus::Sources(_) => "j/k select source · Enter open · Tab follow-ups · Esc home",
        Focus::Followups(_) => "j/k select follow-up · Enter search it · Tab body · Esc home",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_hint_matches_focus() {
        struct Case {
            name: &'static str,
            focus: Focus,
            want_fragment: &'static str,
        }
        let cases = [
            Case {
                name: "body hints scrolling and jumps",
                focus: Focus::Body,
                want_fragment: "1-9 source",
            },
            Case {
                name: "sources hints opening",
                focus: Focus::Sources(0),
                want_fragment: "Enter open",
            },
            Case {
                name: "followups hints searching",
                focus: Focus::Followups(0),
                want_fragment: "Enter search it",
            },
        ];
        for case in cases {
            assert!(
                footer_hint(case.focus).contains(case.want_fragment),
                "{}",
                case.name
            );
        }
    }
}
