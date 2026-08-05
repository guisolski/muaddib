use crate::pipeline::search::FAST_TARGET_SECS;
use crate::tui::app::App;
use crate::tui::search_state::{SearchState, SubQueryState};
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
    if let Some(count) = app.search.web_hits {
        lines.push(Line::styled(format!("web hits: {count}"), theme::dim()));
    }
    if app.search.pages_fetched > 0 {
        lines.push(Line::styled(
            format!("page content: {} pages", app.search.pages_fetched),
            theme::dim(),
        ));
    }
    lines.extend(stage_lines(app));
    lines.push(Line::default());
    lines.push(Line::styled("Esc cancel", theme::dim()));
    lines
}

fn stage_lines(app: &App) -> Vec<Line<'static>> {
    let Some(label) = stage_label(&app.search) else {
        return Vec::new();
    };
    vec![
        Line::default(),
        Line::from(vec![
            Span::styled(spinner::frame(app.tick).to_string(), theme::citation()),
            Span::styled(label, theme::citation()),
        ]),
    ]
}

fn stage_label(search: &SearchState) -> Option<String> {
    if search.reflecting {
        return Some(match search.gaps {
            None => " reviewing the draft…".to_string(),
            Some(0) => " reviewing the draft… no gaps found".to_string(),
            Some(1) => " reviewing the draft… 1 gap found".to_string(),
            Some(count) => format!(" reviewing the draft… {count} gaps found"),
        });
    }
    search
        .synthesizing
        .then(|| " synthesizing answer…".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_line_names_whichever_stage_is_running() {
        struct Case {
            name: &'static str,
            search: SearchState,
            want: Option<&'static str>,
        }
        let cases = [
            Case {
                name: "fanning out shows no stage line at all",
                search: SearchState::default(),
                want: None,
            },
            Case {
                name: "synthesis names itself",
                search: SearchState {
                    synthesizing: true,
                    ..SearchState::default()
                },
                want: Some(" synthesizing answer…"),
            },
            Case {
                name: "the critic is running but has not reported yet",
                search: SearchState {
                    reflecting: true,
                    ..SearchState::default()
                },
                want: Some(" reviewing the draft…"),
            },
            Case {
                name: "a clean review says so instead of showing a zero",
                search: SearchState {
                    reflecting: true,
                    gaps: Some(0),
                    ..SearchState::default()
                },
                want: Some(" reviewing the draft… no gaps found"),
            },
            Case {
                name: "one gap stays singular",
                search: SearchState {
                    reflecting: true,
                    gaps: Some(1),
                    ..SearchState::default()
                },
                want: Some(" reviewing the draft… 1 gap found"),
            },
            Case {
                name: "several gaps are counted",
                search: SearchState {
                    reflecting: true,
                    gaps: Some(3),
                    ..SearchState::default()
                },
                want: Some(" reviewing the draft… 3 gaps found"),
            },
            Case {
                name: "reflection wins while both flags linger",
                search: SearchState {
                    reflecting: true,
                    synthesizing: true,
                    ..SearchState::default()
                },
                want: Some(" reviewing the draft…"),
            },
        ];
        for case in cases {
            assert_eq!(
                stage_label(&case.search).as_deref(),
                case.want,
                "{}",
                case.name
            );
        }
    }
}
