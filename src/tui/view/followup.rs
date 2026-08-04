use crate::core::readability::excerpt;
use crate::tui::app::{App, FollowUpForm};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const MODAL_WIDTH: u16 = 60;
const MODAL_HEIGHT: u16 = 5;

pub fn draw(frame: &mut Frame, app: &App, form: &FollowUpForm) {
    let area = centered_rect(frame.area(), MODAL_WIDTH, MODAL_HEIGHT);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(" follow-up ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parent_query = app
        .tree
        .node(form.parent)
        .map_or_else(String::new, |node| node.query.clone());
    let from = excerpt(
        &format!("from: {parent_query}"),
        usize::from(inner.width.saturating_sub(1)),
    );
    let text_width = usize::from(inner.width.saturating_sub(1));
    let scroll = form.input.visual_scroll(text_width);
    let lines = vec![
        Line::styled(from, theme::dim()),
        Line::default(),
        Line::raw(form.input.value().to_string()),
    ];
    frame.render_widget(
        Paragraph::new(lines).scroll((0, u16::try_from(scroll).unwrap_or(0))),
        inner,
    );
    let offset = form.input.visual_cursor().saturating_sub(scroll);
    let x = inner.x + u16::try_from(offset).unwrap_or(0);
    let y = inner.y + 2;
    if x < inner.right() && y < inner.bottom() {
        frame.set_cursor_position(Position { x, y });
    }
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
