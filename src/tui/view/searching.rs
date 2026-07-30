use crate::tui::app::{App, SubQueryState};
use crate::tui::theme;
use crate::tui::widgets::spinner;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const PANEL_WIDTH: u16 = 70;

pub fn draw(frame: &mut Frame, app: &App) {
    let lines = panel_lines(app);
    let height = lines.len() as u16;
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(frame.area());
    let width = PANEL_WIDTH
        .min(frame.area().width.saturating_sub(4))
        .max(20);
    let [panel] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    frame.render_widget(Paragraph::new(lines), panel);
}

fn panel_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(spinner::frame(app.tick).to_string(), theme::citation()),
        Span::styled(format!(" searching: {}", query_text(app)), theme::title()),
        Span::styled(format!("  {}s", elapsed_secs(app)), theme::dim()),
    ]));
    lines.push(Line::default());
    lines.extend(sub_query_lines(app));
    if app.synthesizing {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled(spinner::frame(app.tick).to_string(), theme::citation()),
            Span::styled(" synthesizing answer…", theme::citation()),
        ]));
    }
    lines.push(Line::default());
    lines.push(Line::styled("Esc cancel", theme::dim()));
    lines
}

fn query_text(app: &App) -> String {
    app.plan.as_ref().map_or_else(
        || app.input.value().to_string(),
        |plan| plan.original.clone(),
    )
}

fn elapsed_secs(app: &App) -> u64 {
    app.started_at
        .map_or(0, |started| started.elapsed().as_secs())
}

fn sub_query_lines(app: &App) -> Vec<Line<'static>> {
    let Some(plan) = &app.plan else {
        return vec![Line::styled("planning sub-queries…", theme::dim())];
    };
    plan.sub_queries
        .iter()
        .enumerate()
        .map(|(idx, sub)| {
            let state = app
                .progress
                .get(idx)
                .copied()
                .unwrap_or(SubQueryState::Pending);
            Line::from(vec![
                state_glyph(state, app.tick),
                Span::styled(format!(" [{}] ", sub.lang), theme::dim()),
                Span::raw(sub.query.clone()),
            ])
        })
        .collect()
}

fn state_glyph(state: SubQueryState, tick: u64) -> Span<'static> {
    match state {
        SubQueryState::Pending => Span::styled("·", theme::dim()),
        SubQueryState::Running => Span::styled(spinner::frame(tick).to_string(), theme::citation()),
        SubQueryState::Done => Span::styled("✓", theme::ok()),
        SubQueryState::Failed => Span::styled("✗", theme::err()),
    }
}
