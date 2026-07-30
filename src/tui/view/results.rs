use crate::core::answer::{Answer, Block, ListItem, Source};
use crate::pipeline::LinkStatus;
use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::widgets::chart::bar_chart_lines;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::HashMap;

pub fn draw(frame: &mut Frame, app: &App) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let Some(answer) = &app.answer else {
        frame.render_widget(
            Paragraph::new(Line::styled("no answer to show", theme::dim())).centered(),
            content,
        );
        return;
    };
    let width = content.width.saturating_sub(4);
    let selected = app.sources_focused.then_some(app.selected_source);
    let lines = answer_lines(answer, width, &app.links, selected);
    let max_scroll = (lines.len() as u16).saturating_sub(content.height);
    let scroll = app.scroll.min(max_scroll);
    let [padded] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(content);
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), padded);
    frame.render_widget(
        Paragraph::new(Line::styled(footer_hint(app), theme::dim())).centered(),
        footer,
    );
}

fn footer_hint(app: &App) -> &'static str {
    if app.sources_focused {
        "j/k select source · Enter open · Tab back to text · Esc home"
    } else {
        "j/k scroll · Tab sources · n new search · / refine · Esc home · q quit"
    }
}

pub fn answer_lines(
    answer: &Answer,
    width: u16,
    links: &HashMap<u32, LinkStatus>,
    selected_source: Option<usize>,
) -> Vec<Line<'static>> {
    let text_width = usize::from(width).max(10);
    let mut lines = vec![
        Line::styled(answer.title.clone(), theme::title()),
        Line::default(),
    ];
    for block in &answer.blocks {
        append_block(&mut lines, block, text_width, width);
    }
    append_sources(&mut lines, &answer.sources, links, selected_source);
    append_followups(&mut lines, &answer.followups);
    lines
}

fn append_block(lines: &mut Vec<Line<'static>>, block: &Block, text_width: usize, width: u16) {
    match block {
        Block::Heading { text, .. } => {
            lines.push(Line::styled(text.clone(), theme::heading()));
            lines.push(Line::default());
        }
        Block::Paragraph { text, source_ids } => {
            append_cited_text(lines, text, source_ids, text_width, "", "");
            lines.push(Line::default());
        }
        Block::List { ordered, items } => {
            for (index, item) in items.iter().enumerate() {
                append_list_item(lines, item, *ordered, index, text_width);
            }
            lines.push(Line::default());
        }
        Block::Quote { text, source_ids } => {
            append_quote(lines, text, source_ids, text_width);
            lines.push(Line::default());
        }
        Block::Table {
            headers,
            rows,
            source_ids,
        } => {
            append_table(lines, headers, rows, source_ids, text_width);
            lines.push(Line::default());
        }
        Block::Chart {
            title,
            labels,
            values,
            unit,
            source_ids,
            ..
        } => {
            lines.push(Line::from(vec![
                Span::styled(title.clone(), theme::heading()),
                citation_span(source_ids),
            ]));
            for row in bar_chart_lines(labels, values, unit, width) {
                lines.push(Line::styled(row, theme::citation()));
            }
            lines.push(Line::default());
        }
        Block::Unknown => {}
    }
}

fn append_cited_text(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    source_ids: &[u32],
    text_width: usize,
    first_prefix: &str,
    rest_prefix: &str,
) {
    let usable = text_width
        .saturating_sub(first_prefix.chars().count())
        .max(4);
    let wrapped = wrap_text(text, usable);
    let last = wrapped.len().saturating_sub(1);
    for (index, row) in wrapped.into_iter().enumerate() {
        let prefix = if index == 0 {
            first_prefix
        } else {
            rest_prefix
        };
        let mut spans = vec![Span::raw(format!("{prefix}{row}"))];
        if index == last {
            spans.push(citation_span(source_ids));
        }
        lines.push(Line::from(spans));
    }
}

fn append_list_item(
    lines: &mut Vec<Line<'static>>,
    item: &ListItem,
    ordered: bool,
    index: usize,
    text_width: usize,
) {
    let bullet = if ordered {
        format!("{}. ", index + 1)
    } else {
        "• ".to_string()
    };
    let indent = " ".repeat(bullet.chars().count());
    append_cited_text(
        lines,
        &item.text,
        &item.source_ids,
        text_width,
        &bullet,
        &indent,
    );
}

fn append_quote(lines: &mut Vec<Line<'static>>, text: &str, source_ids: &[u32], text_width: usize) {
    let wrapped = wrap_text(text, text_width.saturating_sub(2).max(4));
    let last = wrapped.len().saturating_sub(1);
    for (index, row) in wrapped.into_iter().enumerate() {
        let mut spans = vec![Span::styled(format!("│ {row}"), theme::quote())];
        if index == last {
            spans.push(citation_span(source_ids));
        }
        lines.push(Line::from(spans));
    }
}

fn append_table(
    lines: &mut Vec<Line<'static>>,
    headers: &[String],
    rows: &[Vec<String>],
    source_ids: &[u32],
    text_width: usize,
) {
    let table_rows = layout_table(headers, rows, text_width);
    for (index, row) in table_rows.into_iter().enumerate() {
        if index == 0 {
            lines.push(Line::from(vec![
                Span::styled(row, theme::heading()),
                citation_span(source_ids),
            ]));
        } else {
            lines.push(Line::raw(row));
        }
    }
}

fn citation_span(source_ids: &[u32]) -> Span<'static> {
    if source_ids.is_empty() {
        return Span::raw("");
    }
    let markers: String = source_ids.iter().map(|id| format!("[{id}]")).collect();
    Span::styled(format!(" {markers}"), theme::citation())
}

fn append_sources(
    lines: &mut Vec<Line<'static>>,
    sources: &[Source],
    links: &HashMap<u32, LinkStatus>,
    selected: Option<usize>,
) {
    if sources.is_empty() {
        return;
    }
    lines.push(Line::styled("Sources", theme::heading()));
    for (index, source) in sources.iter().enumerate() {
        let mut line = Line::from(vec![
            Span::styled(format!("[{}] ", source.id), theme::citation()),
            link_glyph(links.get(&source.id).copied()),
            Span::raw(format!(" {} — ", source.title)),
            Span::styled(source.url.clone(), theme::dim()),
            Span::styled(format!(" ({})", source.lang), theme::dim()),
        ]);
        if selected == Some(index) {
            line = line.style(theme::selected());
        }
        lines.push(line);
    }
}

fn link_glyph(status: Option<LinkStatus>) -> Span<'static> {
    match status {
        Some(LinkStatus::Valid) => Span::styled("✓", theme::ok()),
        Some(LinkStatus::Invalid(code)) => Span::styled(format!("✗ {code}"), theme::err()),
        Some(LinkStatus::Unreachable) => Span::styled("✗ unreachable", theme::err()),
        None => Span::styled("·", theme::dim()),
    }
}

fn append_followups(lines: &mut Vec<Line<'static>>, followups: &[String]) {
    if followups.is_empty() {
        return;
    }
    lines.push(Line::default());
    for followup in followups {
        lines.push(Line::styled(format!("→ {followup}"), theme::dim()));
    }
}

pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            rows.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

pub fn layout_table(headers: &[String], rows: &[Vec<String>], width: usize) -> Vec<String> {
    if headers.is_empty() && rows.is_empty() {
        return Vec::new();
    }
    let column_count = headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if column_count == 0 {
        return Vec::new();
    }
    let cap = (width / column_count).saturating_sub(2).clamp(4, 30);
    let widths = column_widths(headers, rows, column_count, cap);
    let mut out = Vec::new();
    if !headers.is_empty() {
        out.push(format_row(headers, &widths, cap));
        out.push(separator_row(&widths));
    }
    for row in rows {
        out.push(format_row(row, &widths, cap));
    }
    out
}

fn column_widths(
    headers: &[String],
    rows: &[Vec<String>],
    column_count: usize,
    cap: usize,
) -> Vec<usize> {
    (0..column_count)
        .map(|col| {
            let header_len = headers.get(col).map_or(0, |cell| cell.chars().count());
            let cell_len = rows
                .iter()
                .map(|row| row.get(col).map_or(0, |cell| cell.chars().count()))
                .max()
                .unwrap_or(0);
            header_len.max(cell_len).clamp(1, cap)
        })
        .collect()
}

fn format_row(cells: &[String], widths: &[usize], cap: usize) -> String {
    widths
        .iter()
        .enumerate()
        .map(|(col, col_width)| {
            let cell = cells.get(col).map_or("", String::as_str);
            let clipped: String = cell.chars().take(cap).collect();
            format!("{clipped:<col_width$}")
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn separator_row(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|col_width| "─".repeat(*col_width))
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::ChartType;

    fn text_of(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect()
    }

    fn full_answer() -> Answer {
        Answer {
            title: "Title".to_string(),
            language: "en".to_string(),
            blocks: vec![
                Block::Heading {
                    level: 2,
                    text: "Section".to_string(),
                },
                Block::Paragraph {
                    text: "A claim with backing.".to_string(),
                    source_ids: vec![1, 2],
                },
                Block::List {
                    ordered: true,
                    items: vec![ListItem {
                        text: "first point".to_string(),
                        source_ids: vec![1],
                    }],
                },
                Block::Quote {
                    text: "quoted words".to_string(),
                    source_ids: vec![2],
                },
                Block::Table {
                    headers: vec!["name".to_string(), "value".to_string()],
                    rows: vec![vec!["a".to_string(), "1".to_string()]],
                    source_ids: vec![1],
                },
                Block::Chart {
                    chart_type: ChartType::Bar,
                    title: "Share".to_string(),
                    labels: vec!["x".to_string()],
                    values: vec![5.0],
                    unit: "%".to_string(),
                    source_ids: vec![2],
                },
                Block::Unknown,
            ],
            sources: vec![
                Source {
                    id: 1,
                    title: "One".to_string(),
                    url: "https://one.example".to_string(),
                    lang: "en".to_string(),
                },
                Source {
                    id: 2,
                    title: "Two".to_string(),
                    url: "https://two.example".to_string(),
                    lang: "en".to_string(),
                },
            ],
            followups: vec!["next".to_string()],
        }
    }

    #[test]
    fn every_block_type_renders_expected_shapes() {
        let links = HashMap::from([(1, LinkStatus::Valid), (2, LinkStatus::Invalid(404))]);
        let lines = answer_lines(&full_answer(), 60, &links, None);
        let rendered: Vec<String> = lines.iter().map(text_of).collect();
        let joined = rendered.join("\n");
        assert!(joined.contains("Title"));
        assert!(joined.contains("Section"));
        assert!(joined.contains("A claim with backing. [1][2]"));
        assert!(joined.contains("1. first point [1]"));
        assert!(joined.contains("│ quoted words [2]"));
        assert!(joined.contains("name"));
        assert!(joined.contains("─"));
        assert!(joined.contains("Share [2]"));
        assert!(joined.contains("▇"));
        assert!(joined.contains("[1] ✓ One — https://one.example (en)"));
        assert!(joined.contains("[2] ✗ 404 Two — https://two.example (en)"));
        assert!(joined.contains("→ next"));
    }

    #[test]
    fn unchecked_links_show_a_neutral_glyph() {
        let lines = answer_lines(&full_answer(), 60, &HashMap::new(), None);
        let joined = lines
            .iter()
            .map(|line| text_of(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[1] · One"));
    }

    #[test]
    fn narrow_widths_never_panic() {
        for width in [0u16, 1, 5, 10] {
            let lines = answer_lines(&full_answer(), width, &HashMap::new(), Some(0));
            assert!(!lines.is_empty(), "width {width}");
        }
    }

    #[test]
    fn wrap_text_greedily_fills_lines() {
        struct Case {
            name: &'static str,
            text: &'static str,
            width: usize,
            want: Vec<&'static str>,
        }
        let cases = [
            Case {
                name: "fits on one line",
                text: "short text",
                width: 20,
                want: vec!["short text"],
            },
            Case {
                name: "wraps at word boundaries",
                text: "one two three four",
                width: 9,
                want: vec!["one two", "three", "four"],
            },
            Case {
                name: "long word overflows its own line",
                text: "a verylongword b",
                width: 5,
                want: vec!["a", "verylongword", "b"],
            },
            Case {
                name: "empty text yields one empty row",
                text: "",
                width: 10,
                want: vec![""],
            },
        ];
        for case in cases {
            assert_eq!(wrap_text(case.text, case.width), case.want, "{}", case.name);
        }
    }

    #[test]
    fn layout_table_aligns_columns_and_truncates_cells() {
        let headers = vec!["name".to_string(), "very long header text here".to_string()];
        let rows = vec![vec!["a".to_string(), "1".to_string()]];
        let table = layout_table(&headers, &rows, 40);
        assert_eq!(table.len(), 3);
        assert!(table[0].starts_with("name"));
        assert!(table[1].contains('─'));
        assert!(table[0].chars().count() <= 40);
    }

    #[test]
    fn layout_table_handles_empty_input() {
        assert!(layout_table(&[], &[], 40).is_empty());
    }
}
