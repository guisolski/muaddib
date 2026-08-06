use crate::tui::app::PassphraseForm;
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const MODAL_WIDTH: u16 = 52;
const MODAL_HEIGHT: u16 = 9;

pub fn draw(frame: &mut Frame, form: &PassphraseForm) {
    let area = centered_rect(frame.area(), MODAL_WIDTH, MODAL_HEIGHT);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(title(form));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines(form)), inner);
}

fn title(form: &PassphraseForm) -> &'static str {
    if form.creating {
        " new key vault "
    } else {
        " unlock key vault "
    }
}

pub fn lines(form: &PassphraseForm) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default()];
    lines.push(field_line(
        "passphrase",
        form.input.value(),
        !form.on_confirm,
    ));
    if form.creating {
        lines.push(field_line("confirm", form.confirm.value(), form.on_confirm));
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        form.error
            .clone()
            .unwrap_or_else(|| "keys are sealed with argon2id + xchacha20poly1305".to_string()),
        if form.error.is_some() {
            theme::title()
        } else {
            theme::dim()
        },
    ));
    lines.push(Line::default());
    lines.push(Line::styled(
        "Tab field · Enter confirm · Esc cancel",
        theme::dim(),
    ));
    lines
}

fn field_line(label: &str, value: &str, selected: bool) -> Line<'static> {
    let marker = if selected { "▸ " } else { "  " };
    let label_style = if selected {
        theme::title()
    } else {
        theme::dim()
    };
    Line::from(vec![
        Span::styled(marker.to_string(), theme::citation()),
        Span::styled(format!("{label:<12}"), label_style),
        Span::raw("•".repeat(value.chars().count())),
    ])
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_input::Input;

    fn rendered(form: &PassphraseForm) -> String {
        lines(form)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_typed_passphrase_is_never_rendered() {
        let mut form = PassphraseForm::new(true);
        form.input = Input::new("open sesame".to_string());
        form.confirm = Input::new("open sesame".to_string());
        let text = rendered(&form);
        assert!(!text.contains("open sesame"), "{text}");
        assert!(text.contains(&"•".repeat("open sesame".len())), "{text}");
    }

    #[test]
    fn only_the_unlock_flow_hides_the_confirm_field() {
        struct Case {
            name: &'static str,
            creating: bool,
            want_confirm: bool,
        }
        let cases = [
            Case {
                name: "creating asks twice",
                creating: true,
                want_confirm: true,
            },
            Case {
                name: "unlocking asks once",
                creating: false,
                want_confirm: false,
            },
        ];
        for case in cases {
            let form = PassphraseForm::new(case.creating);
            let fields = rendered(&form)
                .lines()
                .filter(|line| line.contains("passphrase") || line.contains("confirm  "))
                .count();
            assert_eq!(
                fields,
                if case.want_confirm { 2 } else { 1 },
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn an_error_replaces_the_hint() {
        let mut form = PassphraseForm::new(false);
        form.error = Some("wrong passphrase".to_string());
        let text = rendered(&form);
        assert!(text.contains("wrong passphrase"), "{text}");
        assert!(!text.contains("argon2id"), "{text}");
    }

    #[test]
    fn the_title_names_the_flow() {
        assert_eq!(title(&PassphraseForm::new(true)), " new key vault ");
        assert_eq!(title(&PassphraseForm::new(false)), " unlock key vault ");
    }
}
