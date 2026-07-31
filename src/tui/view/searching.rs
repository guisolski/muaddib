use crate::pipeline::search::FAST_TARGET_SECS;
use crate::tui::app::App;
use crate::tui::search_state::SubQueryState;
use crate::tui::theme;
use crate::tui::widgets::{mascot, spinner};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

const PANEL_WIDTH: u16 = 70;
const MASCOT_MIN_HEIGHT: u16 = 14;

pub fn draw(frame: &mut Frame, app: &App) {
    let width = PANEL_WIDTH
        .min(frame.area().width.saturating_sub(4))
        .max(20);
    let include_mascot = frame.area().height >= MASCOT_MIN_HEIGHT;
    let lines = panel_lines(app, include_mascot, usize::from(width));
    let height = lines.len() as u16;
    let [row] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(frame.area());
    let [panel] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    frame.render_widget(Paragraph::new(lines), panel);
}

fn panel_lines(app: &App, include_mascot: bool, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if include_mascot {
        lines.extend(mascot::hop_lines(app.tick, width, app.config.animations));
        lines.push(Line::default());
    }
    let mut header = vec![
        Span::styled(spinner::frame(app.tick).to_string(), theme::citation()),
        Span::styled(format!(" searching: {}", query_text(app)), theme::title()),
    ];
    if app.fast {
        header.push(Span::styled(" ⚡ fast", theme::warn()));
    }
    header.push(elapsed_span(app));
    lines.push(Line::from(header));
    lines.push(Line::default());
    lines.extend(sub_query_lines(app));
    if app.search.synthesizing {
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
    app.search.plan.as_ref().map_or_else(
        || app.input.value().to_string(),
        |plan| plan.original.clone(),
    )
}

fn elapsed_secs(app: &App) -> u64 {
    app.search.elapsed_secs(Instant::now())
}

fn elapsed_span(app: &App) -> Span<'static> {
    let elapsed = elapsed_secs(app);
    let style = if app.fast && elapsed >= FAST_TARGET_SECS {
        theme::warn()
    } else {
        theme::dim()
    };
    Span::styled(format!("  {elapsed}s"), style)
}

fn sub_query_lines(app: &App) -> Vec<Line<'static>> {
    let Some(plan) = &app.search.plan else {
        return vec![Line::styled("planning sub-queries…", theme::dim())];
    };
    plan.sub_queries
        .iter()
        .enumerate()
        .map(|(idx, sub)| {
            let state = app.search.sub_query_state(idx);
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
