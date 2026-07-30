use crate::tui::keymap::{KEYMAP, Scope, key_label};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const SCOPE_ORDER: &[(Scope, &str)] = &[
    (Scope::Global, "Everywhere"),
    (Scope::Home, "Home"),
    (Scope::Results, "Results"),
    (Scope::Modal, "Config modal"),
];

pub fn draw(frame: &mut Frame) {
    let lines = help_lines();
    let height = (lines.len() as u16 + 2).min(frame.area().height);
    let width = 46u16.min(frame.area().width);
    let area = centered_rect(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(" keys ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    let [rect] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    rect
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (scope, title) in SCOPE_ORDER {
        let bindings: Vec<_> = KEYMAP
            .iter()
            .filter(|binding| binding.scope == *scope)
            .collect();
        if bindings.is_empty() {
            continue;
        }
        lines.push(Line::styled((*title).to_string(), theme::heading()));
        for binding in bindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<10}", key_label(binding.code, binding.mods)),
                    theme::citation(),
                ),
                Span::raw(binding.help.to_string()),
            ]));
        }
    }
    lines
}
