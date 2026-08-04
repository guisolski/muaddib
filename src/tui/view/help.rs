use crate::tui::keymap::{KEYMAP, KeyBinding, Scope, key_label};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const SCOPE_ORDER: &[(Scope, &str)] = &[
    (Scope::Global, "Everywhere"),
    (Scope::Home, "Home"),
    (Scope::Results, "Results"),
    (Scope::Tree, "Research tree"),
    (Scope::FollowUp, "Follow-up"),
    (Scope::Modal, "Config modal"),
];

pub fn draw(frame: &mut Frame, scroll: u16) {
    let lines = help_lines();
    let total = lines.len() as u16;
    let height = (total + 2).min(frame.area().height);
    let width = 46u16.min(frame.area().width);
    let area = centered_rect(frame.area(), width, height);
    frame.render_widget(Clear, area);
    let scroll = scroll.min(max_scroll(frame.area().height));
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(" keys ");
    let block = if total + 2 > height {
        block.title_bottom(Line::styled(" \u{2191}/\u{2193} scroll ", theme::dim()))
    } else {
        block
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

pub fn max_scroll(frame_height: u16) -> u16 {
    let total = help_lines().len() as u16;
    let visible = frame_height.saturating_sub(2);
    total.saturating_sub(visible)
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
        for (label, help) in merged_rows(&bindings) {
            lines.push(Line::from(vec![
                Span::styled(format!("  {label:<10}"), theme::citation()),
                Span::raw(help),
            ]));
        }
    }
    lines
}

fn merged_rows(bindings: &[&'static KeyBinding]) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String, crate::tui::keymap::Action)> = Vec::new();
    for binding in bindings {
        let label = key_label(binding.code, binding.mods);
        match rows.last_mut() {
            Some((merged, help, action)) if *action == binding.action && help == binding.help => {
                merged.push('/');
                merged.push_str(&label);
            }
            _ => rows.push((label, binding.help.to_string(), binding.action)),
        }
    }
    rows.into_iter()
        .map(|(label, help, _)| (label, help))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered_text() -> Vec<String> {
        help_lines()
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn help_lists_every_scope_with_bindings() {
        let text = rendered_text().join("\n");
        for (_, title) in SCOPE_ORDER {
            assert!(text.contains(title), "{title}");
        }
        assert!(text.contains("research tree"));
        assert!(text.contains("follow-up search"));
        assert!(text.contains("save session"));
        assert!(text.contains("branch from node"));
    }

    #[test]
    fn same_action_keys_merge_into_one_row() {
        let text = rendered_text().join("\n");
        assert!(text.contains("j/Down"));
        assert!(text.contains("k/Up"));
        let scroll_rows = rendered_text()
            .iter()
            .filter(|line| line.trim_start().starts_with("j/Down"))
            .count();
        assert_eq!(scroll_rows, 2);
    }

    #[test]
    fn max_scroll_covers_the_overflow_and_settles_at_zero_when_it_fits() {
        let total = help_lines().len() as u16;
        assert_eq!(max_scroll(total + 2), 0);
        assert_eq!(max_scroll(total), 2);
        assert!(max_scroll(24) > 0);
    }
}
