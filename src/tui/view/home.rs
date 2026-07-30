use crate::core::mode::MODES;
use crate::tui::app::App;
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};

const MAX_INPUT_WIDTH: u16 = 64;

pub fn draw(frame: &mut Frame, app: &App) {
    let [
        wordmark_row,
        gap,
        input_row,
        modes_row,
        gap2,
        notice_row,
        status_row,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .flex(Flex::Center)
    .areas(frame.area());
    let _ = (gap, gap2);
    frame.render_widget(wordmark_line(), wordmark_row);
    draw_input(frame, app, input_row);
    frame.render_widget(Paragraph::new(modes_line(app)).centered(), modes_row);
    if let Some(notice) = &app.notice {
        frame.render_widget(
            Paragraph::new(Line::styled(notice.clone(), theme::warn())).centered(),
            notice_row,
        );
    }
    frame.render_widget(Paragraph::new(status_line(app)).centered(), status_row);
    frame.render_widget(Paragraph::new(hints_line()).centered(), hints_row);
}

fn wordmark_line() -> Paragraph<'static> {
    Paragraph::new(Line::styled("▲ faro", theme::title())).centered()
}

fn draw_input(frame: &mut Frame, app: &App, row: Rect) {
    let width = MAX_INPUT_WIDTH.min(row.width.saturating_sub(4)).max(10);
    let [input_area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    let inner_width = usize::from(input_area.width.saturating_sub(2));
    let scroll = app.input.visual_scroll(inner_width);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation());
    let paragraph = Paragraph::new(app.input.value())
        .scroll((0, scroll as u16))
        .block(block);
    frame.render_widget(paragraph, input_area);
    let cursor_x = input_area.x + 1 + (app.input.visual_cursor().saturating_sub(scroll)) as u16;
    frame.set_cursor_position((
        cursor_x.min(input_area.right().saturating_sub(2)),
        input_area.y + 1,
    ));
}

fn modes_line(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, spec) in MODES.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(" · ", theme::dim()));
        }
        let style = if index == app.mode_idx % MODES.len() {
            theme::title().add_modifier(Modifier::UNDERLINED)
        } else {
            theme::dim()
        };
        spans.push(Span::styled(spec.label, style));
    }
    Line::from(spans)
}

fn status_line(app: &App) -> Line<'static> {
    let engine_available = app.selected_engine().is_some_and(|status| status.available);
    let dot_style = if engine_available {
        theme::ok()
    } else {
        theme::err()
    };
    let model = app
        .config
        .model_override(&app.config.engine)
        .unwrap_or("default");
    Line::from(vec![
        Span::styled(format!("{} ", app.config.engine), theme::dim()),
        Span::styled("●", dot_style),
        Span::styled(
            format!(" · {model} · {}", app.config.language),
            theme::dim(),
        ),
    ])
}

fn hints_line() -> Line<'static> {
    Line::styled(
        "Enter search · Tab mode · Ctrl+O config · Ctrl+G help",
        theme::dim(),
    )
}
