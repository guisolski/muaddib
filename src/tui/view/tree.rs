use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::widgets::tree::tree_lines;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

pub fn draw(frame: &mut Frame, app: &App) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let width = usize::from(content.width.saturating_sub(4));
    let rows = tree_lines(&app.tree, width);
    let mut lines = vec![
        Line::styled("research tree", theme::title()),
        Line::default(),
    ];
    for (idx, text) in rows.into_iter().enumerate() {
        let style = if idx == app.tree_sel {
            theme::selected()
        } else {
            theme::dim()
        };
        lines.push(Line::styled(format!("  {text}"), style));
    }
    let offset = scroll_offset(app.tree_sel, lines.len(), usize::from(content.height));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(offset).unwrap_or(u16::MAX), 0)),
        content,
    );
    frame.render_widget(
        Line::styled(
            "Enter view \u{00b7} f branch \u{00b7} s save session \u{00b7} Esc back",
            theme::dim(),
        ),
        footer,
    );
}

fn scroll_offset(selected: usize, total_lines: usize, height: usize) -> usize {
    let selected_line = selected + 2;
    if height == 0 || selected_line < height {
        return 0;
    }
    (selected_line + 1 - height).min(total_lines.saturating_sub(height))
}
