use crate::core::mode::MODES;
use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::widgets::mascot;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};

const MAX_INPUT_WIDTH: u16 = 64;
const MIN_INPUT_WIDTH: u16 = 10;
const SCENE_ROWS: u16 = 5;
const FULL_LAYOUT_ROWS: u16 = 14;

pub fn draw(frame: &mut Frame, app: &App) {
    let wordmark_height = wordmark_rows(frame.area().height);
    let [
        wordmark_row,
        gap,
        input_row,
        modes_row,
        fast_row,
        notice_row,
        status_row,
        hints_row,
    ] = Layout::vertical([
        Constraint::Length(wordmark_height),
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
    let _ = gap;
    if wordmark_height == SCENE_ROWS {
        frame.render_widget(scene_wordmark(app), wordmark_row);
    } else {
        frame.render_widget(wordmark_line(), wordmark_row);
    }
    draw_input(frame, app, input_row);
    frame.render_widget(Paragraph::new(modes_line(app)).centered(), modes_row);
    frame.render_widget(Paragraph::new(fast_line(app)).centered(), fast_row);
    if let Some(notice) = &app.notice {
        frame.render_widget(
            Paragraph::new(Line::styled(notice.clone(), theme::warn())).centered(),
            notice_row,
        );
    }
    frame.render_widget(Paragraph::new(status_line(app)).centered(), status_row);
    frame.render_widget(Paragraph::new(hints_line()).centered(), hints_row);
}

fn wordmark_rows(height: u16) -> u16 {
    if height >= FULL_LAYOUT_ROWS {
        SCENE_ROWS
    } else {
        1
    }
}

fn scene_wordmark(app: &App) -> Paragraph<'static> {
    let scene = mascot::home_scene(mascot::mascot_state(app), app.tick, app.config.animations);
    let mut lines = scene.to_vec();
    lines.push(Line::styled("muaddib", theme::title()));
    Paragraph::new(lines).centered()
}

fn wordmark_line() -> Paragraph<'static> {
    Paragraph::new(Line::styled("▲ muaddib", theme::title())).centered()
}

fn draw_input(frame: &mut Frame, app: &App, row: Rect) {
    let input_area = input_box(row);
    let scroll = app.input.visual_scroll(text_width(input_area));
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation());
    let paragraph = Paragraph::new(app.input.value())
        .scroll((0, clamp_u16(scroll)))
        .block(block);
    frame.render_widget(paragraph, input_area);
    if app.overlay.is_some() {
        return;
    }
    let offset = app.input.visual_cursor().saturating_sub(scroll);
    if let Some(position) = cursor_cell(input_area, offset) {
        frame.set_cursor_position(position);
    }
}

fn input_box(row: Rect) -> Rect {
    let width = MAX_INPUT_WIDTH
        .min(row.width.saturating_sub(4))
        .max(MIN_INPUT_WIDTH)
        .min(row.width);
    let [input_area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(row);
    input_area
}

fn text_width(input_area: Rect) -> usize {
    usize::from(input_area.width.saturating_sub(3))
}

fn cursor_cell(input_area: Rect, visual_offset: usize) -> Option<(u16, u16)> {
    let inner = input_area.inner(Margin::new(1, 1));
    if inner.is_empty() {
        return None;
    }
    let column = inner
        .x
        .saturating_add(clamp_u16(visual_offset))
        .min(inner.right().saturating_sub(1));
    Some((column, inner.y))
}

fn clamp_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

struct ModesLayout {
    separator: &'static str,
    label_chars: Option<usize>,
}

const MODES_LAYOUTS: &[ModesLayout] = &[
    ModesLayout {
        separator: " · ",
        label_chars: None,
    },
    ModesLayout {
        separator: "·",
        label_chars: None,
    },
    ModesLayout {
        separator: "·",
        label_chars: Some(4),
    },
];

fn abbreviate(label: &str, layout: &ModesLayout) -> String {
    match layout.label_chars {
        Some(max) => label.chars().take(max).collect(),
        None => label.to_string(),
    }
}

fn layout_width(layout: &ModesLayout) -> usize {
    let labels: usize = MODES
        .iter()
        .map(|spec| abbreviate(spec.label, layout).chars().count())
        .sum();
    labels + layout.separator.chars().count() * MODES.len().saturating_sub(1)
}

fn modes_layout(width: u16) -> &'static ModesLayout {
    MODES_LAYOUTS
        .iter()
        .find(|layout| layout_width(layout) <= usize::from(width))
        .unwrap_or(&MODES_LAYOUTS[MODES_LAYOUTS.len() - 1])
}

fn modes_line(app: &App) -> Line<'static> {
    let layout = modes_layout(app.viewport.width);
    let mut spans = Vec::new();
    for (index, spec) in MODES.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(layout.separator, theme::dim()));
        }
        let style = if index == app.mode_idx % MODES.len() {
            theme::title().add_modifier(Modifier::UNDERLINED)
        } else {
            theme::dim()
        };
        spans.push(Span::styled(abbreviate(spec.label, layout), style));
    }
    Line::from(spans)
}

fn fast_line(app: &App) -> Line<'static> {
    if app.fast {
        Line::styled("⚡ fast", theme::warn())
    } else {
        Line::styled("Ctrl+F fast", theme::dim())
    }
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
        "Enter search · ↑ history · Tab mode · Ctrl+O config · Ctrl+G help",
        theme::dim(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_of(x: u16, y: u16, width: u16, height: u16) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn the_modes_row_degrades_instead_of_overflowing() {
        struct Case {
            name: &'static str,
            width: u16,
            want_separator: &'static str,
            want_abbreviated: bool,
        }
        let cases = [
            Case {
                name: "roomy terminal keeps spaced separators",
                width: 100,
                want_separator: " · ",
                want_abbreviated: false,
            },
            Case {
                name: "medium terminal drops the padding first",
                width: 45,
                want_separator: "·",
                want_abbreviated: false,
            },
            Case {
                name: "narrow terminal abbreviates the labels",
                width: 30,
                want_separator: "·",
                want_abbreviated: true,
            },
            Case {
                name: "absurdly narrow still yields the tightest layout",
                width: 1,
                want_separator: "·",
                want_abbreviated: true,
            },
        ];
        for case in cases {
            let layout = modes_layout(case.width);
            assert_eq!(layout.separator, case.want_separator, "{}", case.name);
            assert_eq!(
                layout.label_chars.is_some(),
                case.want_abbreviated,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn every_modes_layout_fits_the_width_it_claims() {
        for width in [100_u16, 45, 30] {
            let layout = modes_layout(width);
            assert!(layout_width(layout) <= usize::from(width), "width {width}");
        }
    }

    #[test]
    fn abbreviated_labels_stay_unique() {
        let layout = &MODES_LAYOUTS[MODES_LAYOUTS.len() - 1];
        let labels: std::collections::BTreeSet<String> = MODES
            .iter()
            .map(|spec| abbreviate(spec.label, layout))
            .collect();
        assert_eq!(labels.len(), MODES.len());
    }

    #[test]
    fn the_wordmark_grows_only_when_the_scene_fits() {
        struct Case {
            name: &'static str,
            height: u16,
            want: u16,
        }
        let cases = [
            Case {
                name: "roomy terminal",
                height: 30,
                want: SCENE_ROWS,
            },
            Case {
                name: "exact fit",
                height: FULL_LAYOUT_ROWS,
                want: SCENE_ROWS,
            },
            Case {
                name: "one row short",
                height: FULL_LAYOUT_ROWS - 1,
                want: 1,
            },
            Case {
                name: "tiny",
                height: 6,
                want: 1,
            },
            Case {
                name: "minimal",
                height: 3,
                want: 1,
            },
        ];
        for case in cases {
            assert_eq!(wordmark_rows(case.height), case.want, "{}", case.name);
        }
    }

    #[test]
    fn cursor_never_leaves_the_inner_rectangle() {
        struct Case {
            name: &'static str,
            input_area: Rect,
            offset: usize,
            want: Option<(u16, u16)>,
        }
        let cases = [
            Case {
                name: "empty input sits on the first inner column",
                input_area: box_of(4, 2, 20, 3),
                offset: 0,
                want: Some((5, 3)),
            },
            Case {
                name: "offset advances one column at a time",
                input_area: box_of(4, 2, 20, 3),
                offset: 7,
                want: Some((12, 3)),
            },
            Case {
                name: "the last inner column is the ceiling",
                input_area: box_of(4, 2, 20, 3),
                offset: 17,
                want: Some((22, 3)),
            },
            Case {
                name: "a runaway offset still stops before the border",
                input_area: box_of(4, 2, 20, 3),
                offset: 9_999,
                want: Some((22, 3)),
            },
            Case {
                name: "a squeezed two row box has no inner row",
                input_area: box_of(4, 2, 20, 2),
                offset: 3,
                want: None,
            },
            Case {
                name: "a single row box has no inner row",
                input_area: box_of(4, 2, 20, 1),
                offset: 0,
                want: None,
            },
            Case {
                name: "a collapsed box has no inner cell",
                input_area: box_of(4, 2, 0, 0),
                offset: 0,
                want: None,
            },
            Case {
                name: "a two column box has no inner column",
                input_area: box_of(4, 2, 2, 3),
                offset: 0,
                want: None,
            },
            Case {
                name: "a three column box holds exactly one inner column",
                input_area: box_of(4, 2, 3, 3),
                offset: 5,
                want: Some((5, 3)),
            },
        ];
        for case in cases {
            assert_eq!(
                cursor_cell(case.input_area, case.offset),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn cursor_stays_inside_a_tall_box() {
        let inner_row = cursor_cell(box_of(0, 0, 20, 9), 0).map(|(_, y)| y);
        assert_eq!(inner_row, Some(1));
    }

    #[test]
    fn text_width_reserves_the_borders_and_the_cursor_column() {
        struct Case {
            name: &'static str,
            width: u16,
            want: usize,
        }
        let cases = [
            Case {
                name: "typical box",
                width: 64,
                want: 61,
            },
            Case {
                name: "minimum box",
                width: MIN_INPUT_WIDTH,
                want: 7,
            },
            Case {
                name: "degenerate box never underflows",
                width: 1,
                want: 0,
            },
        ];
        for case in cases {
            let area = box_of(0, 0, case.width, 3);
            assert_eq!(text_width(area), case.want, "{}", case.name);
        }
    }

    #[test]
    fn input_box_never_exceeds_its_row() {
        struct Case {
            name: &'static str,
            row_width: u16,
            want: u16,
        }
        let cases = [
            Case {
                name: "wide terminal caps at the maximum",
                row_width: 200,
                want: MAX_INPUT_WIDTH,
            },
            Case {
                name: "medium terminal leaves a margin",
                row_width: 40,
                want: 36,
            },
            Case {
                name: "narrow terminal falls back to the minimum",
                row_width: 12,
                want: MIN_INPUT_WIDTH,
            },
            Case {
                name: "tiny terminal never overflows the row",
                row_width: 6,
                want: 6,
            },
            Case {
                name: "zero width row stays empty",
                row_width: 0,
                want: 0,
            },
        ];
        for case in cases {
            let row = box_of(0, 0, case.row_width, 3);
            let area = input_box(row);
            assert_eq!(area.width, case.want, "{}", case.name);
            assert!(area.right() <= row.right(), "{}", case.name);
        }
    }
}
