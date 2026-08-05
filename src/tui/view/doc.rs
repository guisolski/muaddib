use crate::core::answer::{Answer, Block, ListItem, Source};
use crate::pipeline::LinkStatus;
use crate::tui::search_state::ImageFetch;
use crate::tui::theme;
use crate::tui::widgets::chart::bar_chart_lines;
use crate::tui::widgets::diagram::diagram_lines;
use crate::tui::widgets::textwrap::wrap_text;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocSelection {
    None,
    Source(usize),
    Followup(usize),
}

pub const IMAGE_ROWS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSlot {
    pub url: String,
    pub range: LineRange,
}

#[derive(Debug)]
pub struct RenderedDoc {
    pub lines: Vec<Line<'static>>,
    pub block_ranges: Vec<LineRange>,
    pub source_ranges: Vec<LineRange>,
    pub followup_ranges: Vec<LineRange>,
    pub image_slots: Vec<ImageSlot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocAnim {
    pub revealed_blocks: usize,
    pub growth: Vec<f64>,
    pub block_overlays: Vec<Option<Style>>,
    pub source_overlay: Option<(usize, Style)>,
}

impl DocAnim {
    pub fn settled(block_count: usize) -> Self {
        Self {
            revealed_blocks: block_count,
            growth: vec![1.0; block_count],
            block_overlays: vec![None; block_count],
            source_overlay: None,
        }
    }
}

pub fn content_width(area_width: u16) -> u16 {
    area_width.saturating_sub(4)
}

pub fn content_height(area_height: u16) -> u16 {
    area_height.saturating_sub(1)
}

pub fn visible_rows(
    range: LineRange,
    scroll: u16,
    viewport_lines: u16,
) -> Option<(u16, u16, bool)> {
    let top = i64::try_from(range.start).unwrap_or(i64::MAX) - i64::from(scroll);
    let bottom = i64::try_from(range.end).unwrap_or(i64::MAX) - i64::from(scroll);
    let visible_top = top.max(0);
    let visible_bottom = bottom.min(i64::from(viewport_lines));
    if visible_bottom <= visible_top {
        return None;
    }
    let offset = u16::try_from(visible_top).unwrap_or(u16::MAX);
    let height = u16::try_from(visible_bottom - visible_top).unwrap_or(u16::MAX);
    Some((offset, height, top < 0))
}

pub fn scroll_into_view(scroll: u16, range: LineRange, viewport_lines: u16) -> u16 {
    let height = usize::from(viewport_lines);
    if height == 0 {
        return scroll;
    }
    let top = usize::from(scroll);
    let length = range.end.saturating_sub(range.start);
    if range.start < top || length > height {
        return u16::try_from(range.start).unwrap_or(u16::MAX);
    }
    if range.end > top + height {
        return u16::try_from(range.end - height).unwrap_or(u16::MAX);
    }
    scroll
}

pub fn render_doc(
    answer: &Answer,
    width: u16,
    links: &HashMap<u32, LinkStatus>,
    selection: DocSelection,
    anim: &DocAnim,
    images: &HashMap<String, ImageFetch>,
) -> RenderedDoc {
    let text_width = usize::from(width).max(10);
    let mut lines = vec![
        Line::styled(answer.title.clone(), theme::title()),
        Line::default(),
    ];
    let mut block_ranges = Vec::with_capacity(answer.blocks.len());
    let mut image_slots = Vec::new();
    for (index, block) in answer.blocks.iter().enumerate() {
        let start = lines.len();
        if index < anim.revealed_blocks {
            let fraction = anim.growth.get(index).copied().unwrap_or(1.0);
            if let Some(slot) = append_block(&mut lines, block, text_width, width, fraction, images)
            {
                image_slots.push(slot);
            }
            if let Some(overlay) = anim.block_overlays.get(index).copied().flatten() {
                for line in &mut lines[start..] {
                    *line = std::mem::take(line).patch_style(overlay);
                }
            }
        }
        let end = lines.len();
        if end > start {
            lines.push(Line::default());
        }
        block_ranges.push(LineRange { start, end });
    }
    let all_revealed = anim.revealed_blocks >= answer.blocks.len();
    let source_ranges = if all_revealed {
        append_sources(
            &mut lines,
            &answer.sources,
            links,
            selection,
            anim.source_overlay,
        )
    } else {
        Vec::new()
    };
    let followup_ranges = if all_revealed {
        append_followups(&mut lines, &answer.followups, selection)
    } else {
        Vec::new()
    };
    RenderedDoc {
        lines,
        block_ranges,
        source_ranges,
        followup_ranges,
        image_slots,
    }
}

fn append_block(
    lines: &mut Vec<Line<'static>>,
    block: &Block,
    text_width: usize,
    width: u16,
    growth: f64,
    images: &HashMap<String, ImageFetch>,
) -> Option<ImageSlot> {
    match block {
        Block::Heading { text, .. } => {
            lines.push(Line::styled(text.clone(), theme::heading()));
        }
        Block::Paragraph {
            text, source_ids, ..
        } => {
            append_cited_text(lines, text, source_ids, text_width, "", "");
        }
        Block::List { ordered, items, .. } => {
            for (index, item) in items.iter().enumerate() {
                append_list_item(lines, item, *ordered, index, text_width);
            }
        }
        Block::Quote {
            text, source_ids, ..
        } => {
            append_quote(lines, text, source_ids, text_width);
        }
        Block::Table {
            headers,
            rows,
            source_ids,
            ..
        } => {
            append_table(lines, headers, rows, source_ids, text_width);
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
            for row in bar_chart_lines(labels, values, unit, width, growth) {
                lines.push(Line::styled(row, theme::citation()));
            }
        }
        Block::Diagram {
            diagram_type,
            title,
            items,
            source_ids,
            ..
        } => {
            lines.push(Line::from(vec![
                Span::styled(title.clone(), theme::heading()),
                citation_span(source_ids),
            ]));
            for row in diagram_lines(*diagram_type, items, text_width, growth) {
                lines.push(Line::raw(row));
            }
        }
        Block::Image {
            url,
            caption,
            source_ids,
            ..
        } => {
            return Some(append_image(lines, url, caption, source_ids, images));
        }
        Block::Unknown => {}
    }
    None
}

fn append_image(
    lines: &mut Vec<Line<'static>>,
    url: &str,
    caption: &str,
    source_ids: &[u32],
    images: &HashMap<String, ImageFetch>,
) -> ImageSlot {
    lines.push(Line::from(vec![
        Span::styled(image_caption(caption), theme::heading()),
        citation_span(source_ids),
    ]));
    let start = lines.len();
    lines.push(Line::styled(
        image_status_row(images.get(url)),
        theme::dim(),
    ));
    for _ in 1..IMAGE_ROWS {
        lines.push(Line::raw(""));
    }
    ImageSlot {
        url: url.to_string(),
        range: LineRange {
            start,
            end: lines.len(),
        },
    }
}

fn image_caption(caption: &str) -> String {
    let caption = caption.trim();
    if caption.is_empty() {
        "▨".to_string()
    } else {
        format!("▨ {caption}")
    }
}

fn image_status_row(state: Option<&ImageFetch>) -> String {
    match state {
        None => "fetching image…".to_string(),
        Some(ImageFetch::Failed) => "image unavailable".to_string(),
        Some(ImageFetch::Ready(_)) => String::new(),
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
    selection: DocSelection,
    overlay: Option<(usize, Style)>,
) -> Vec<LineRange> {
    if sources.is_empty() {
        return Vec::new();
    }
    lines.push(Line::styled("Sources", theme::heading()));
    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let start = lines.len();
            let mut line = Line::from(vec![
                Span::styled(format!("[{}] ", source.id), theme::citation()),
                link_glyph(links.get(&source.id).copied()),
                Span::raw(format!(" {} — ", source.title)),
                Span::styled(source.url.clone(), theme::dim()),
                Span::styled(format!(" ({})", source.lang), theme::dim()),
            ]);
            if selection == DocSelection::Source(index) {
                line = line.style(theme::selected());
            }
            if let Some((target, style)) = overlay
                && target == index
            {
                line = line.patch_style(style);
            }
            lines.push(line);
            LineRange {
                start,
                end: start + 1,
            }
        })
        .collect()
}

fn link_glyph(status: Option<LinkStatus>) -> Span<'static> {
    match status {
        Some(LinkStatus::Valid) => Span::styled("✓", theme::ok()),
        Some(LinkStatus::Invalid(code)) => Span::styled(format!("✗ {code}"), theme::err()),
        Some(LinkStatus::Unreachable) => Span::styled("✗ unreachable", theme::err()),
        None => Span::styled("·", theme::dim()),
    }
}

fn append_followups(
    lines: &mut Vec<Line<'static>>,
    followups: &[String],
    selection: DocSelection,
) -> Vec<LineRange> {
    if followups.is_empty() {
        return Vec::new();
    }
    lines.push(Line::default());
    followups
        .iter()
        .enumerate()
        .map(|(index, followup)| {
            let start = lines.len();
            let mut line = Line::styled(format!("→ {followup}"), theme::dim());
            if selection == DocSelection::Followup(index) {
                line = line.style(theme::selected());
            }
            lines.push(line);
            LineRange {
                start,
                end: start + 1,
            }
        })
        .collect()
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
    use crate::core::answer::{
        Block, ChartType, DiagramItem, DiagramType, Emphasis, ListItem, Source,
    };
    use ratatui::style::Modifier;

    fn text_of(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect()
    }

    fn render_settled(
        answer: &Answer,
        width: u16,
        links: &HashMap<u32, LinkStatus>,
        selection: DocSelection,
    ) -> RenderedDoc {
        render_doc(
            answer,
            width,
            links,
            selection,
            &DocAnim::settled(answer.blocks.len()),
            &HashMap::new(),
        )
    }

    fn full_answer() -> Answer {
        Answer {
            title: "Title".to_string(),
            language: "en".to_string(),
            blocks: vec![
                Block::Heading {
                    level: 2,
                    text: "Section".to_string(),
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "A claim with backing.".to_string(),
                    source_ids: vec![1, 2],
                    emphasis: Emphasis::None,
                },
                Block::List {
                    ordered: true,
                    items: vec![ListItem {
                        text: "first point".to_string(),
                        source_ids: vec![1],
                    }],
                    emphasis: Emphasis::None,
                },
                Block::Quote {
                    text: "quoted words".to_string(),
                    source_ids: vec![2],
                    emphasis: Emphasis::None,
                },
                Block::Table {
                    headers: vec!["name".to_string(), "value".to_string()],
                    rows: vec![vec!["a".to_string(), "1".to_string()]],
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                Block::Chart {
                    chart_type: ChartType::Bar,
                    title: "Share".to_string(),
                    labels: vec!["x".to_string()],
                    values: vec![5.0],
                    unit: "%".to_string(),
                    source_ids: vec![2],
                    emphasis: Emphasis::None,
                },
                Block::Diagram {
                    diagram_type: DiagramType::Flow,
                    title: "Pipeline".to_string(),
                    items: vec![
                        DiagramItem {
                            label: "Expand".to_string(),
                            detail: "one call".to_string(),
                        },
                        DiagramItem {
                            label: "Search".to_string(),
                            detail: String::new(),
                        },
                    ],
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                Block::Image {
                    url: "https://one.example/figure.png".to_string(),
                    caption: "Figure".to_string(),
                    source_ids: vec![1],
                    emphasis: Emphasis::None,
                },
                Block::Unknown,
            ],
            sources: vec![
                Source {
                    id: 1,
                    title: "One".to_string(),
                    url: "https://one.example".to_string(),
                    lang: "en".to_string(),
                    status: None,
                },
                Source {
                    id: 2,
                    title: "Two".to_string(),
                    url: "https://two.example".to_string(),
                    lang: "en".to_string(),
                    status: None,
                },
            ],
            followups: vec!["next".to_string()],
        }
    }

    #[test]
    fn block_ranges_cover_every_block_in_order() {
        struct Case {
            name: &'static str,
            answer: Answer,
            want_empty: &'static [usize],
        }
        let cases = [
            Case {
                name: "full answer",
                answer: full_answer(),
                want_empty: &[8],
            },
            Case {
                name: "unknown block yields empty range",
                answer: Answer {
                    blocks: vec![Block::Unknown],
                    ..full_answer()
                },
                want_empty: &[0],
            },
            Case {
                name: "no blocks",
                answer: Answer {
                    blocks: Vec::new(),
                    ..full_answer()
                },
                want_empty: &[],
            },
        ];
        for case in cases {
            let doc = render_settled(&case.answer, 60, &HashMap::new(), DocSelection::None);
            assert_eq!(
                doc.block_ranges.len(),
                case.answer.blocks.len(),
                "{}",
                case.name
            );
            let mut previous_end = 0;
            for (index, range) in doc.block_ranges.iter().enumerate() {
                assert!(range.start >= previous_end, "{} block {index}", case.name);
                assert!(range.end >= range.start, "{} block {index}", case.name);
                assert!(range.end <= doc.lines.len(), "{} block {index}", case.name);
                assert_eq!(
                    range.start == range.end,
                    case.want_empty.contains(&index),
                    "{} block {index}",
                    case.name
                );
                previous_end = range.end;
            }
        }
    }

    #[test]
    fn source_and_followup_ranges_point_at_their_lines() {
        let doc = render_settled(&full_answer(), 60, &HashMap::new(), DocSelection::None);
        assert_eq!(doc.source_ranges.len(), 2);
        assert_eq!(doc.followup_ranges.len(), 1);
        assert!(text_of(&doc.lines[doc.source_ranges[0].start]).starts_with("[1]"));
        assert!(text_of(&doc.lines[doc.source_ranges[1].start]).starts_with("[2]"));
        assert_eq!(text_of(&doc.lines[doc.followup_ranges[0].start]), "→ next");
    }

    #[test]
    fn selection_highlights_exactly_one_line() {
        struct Case {
            name: &'static str,
            selection: DocSelection,
            want: fn(&RenderedDoc) -> Option<usize>,
        }
        let cases = [
            Case {
                name: "first source",
                selection: DocSelection::Source(0),
                want: |doc| Some(doc.source_ranges[0].start),
            },
            Case {
                name: "second source",
                selection: DocSelection::Source(1),
                want: |doc| Some(doc.source_ranges[1].start),
            },
            Case {
                name: "first followup",
                selection: DocSelection::Followup(0),
                want: |doc| Some(doc.followup_ranges[0].start),
            },
            Case {
                name: "no selection",
                selection: DocSelection::None,
                want: |_| None,
            },
        ];
        for case in cases {
            let doc = render_settled(&full_answer(), 60, &HashMap::new(), case.selection);
            let reversed: Vec<usize> = doc
                .lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.style.add_modifier.contains(Modifier::REVERSED))
                .map(|(index, _)| index)
                .collect();
            let want: Vec<usize> = (case.want)(&doc).into_iter().collect();
            assert_eq!(reversed, want, "{}", case.name);
        }
    }

    #[test]
    fn content_dimensions_match_draw_layout() {
        struct Case {
            name: &'static str,
            given: u16,
            apply: fn(u16) -> u16,
            want: u16,
        }
        let cases = [
            Case {
                name: "width subtracts horizontal padding",
                given: 84,
                apply: content_width,
                want: 80,
            },
            Case {
                name: "narrow width saturates to zero",
                given: 3,
                apply: content_width,
                want: 0,
            },
            Case {
                name: "height reserves the footer row",
                given: 24,
                apply: content_height,
                want: 23,
            },
            Case {
                name: "zero height saturates to zero",
                given: 0,
                apply: content_height,
                want: 0,
            },
        ];
        for case in cases {
            assert_eq!((case.apply)(case.given), case.want, "{}", case.name);
        }
    }

    #[test]
    fn scroll_into_view_moves_only_when_needed() {
        struct Case {
            name: &'static str,
            scroll: u16,
            range: LineRange,
            viewport_lines: u16,
            want: u16,
        }
        let cases = [
            Case {
                name: "range above viewport scrolls up to its start",
                scroll: 10,
                range: LineRange { start: 2, end: 3 },
                viewport_lines: 5,
                want: 2,
            },
            Case {
                name: "range below viewport scrolls its end into view",
                scroll: 0,
                range: LineRange { start: 10, end: 11 },
                viewport_lines: 4,
                want: 7,
            },
            Case {
                name: "visible range leaves scroll unchanged",
                scroll: 2,
                range: LineRange { start: 3, end: 4 },
                viewport_lines: 5,
                want: 2,
            },
            Case {
                name: "range taller than viewport snaps to its start",
                scroll: 0,
                range: LineRange { start: 2, end: 10 },
                viewport_lines: 4,
                want: 2,
            },
            Case {
                name: "zero height viewport keeps scroll",
                scroll: 5,
                range: LineRange { start: 0, end: 1 },
                viewport_lines: 0,
                want: 5,
            },
        ];
        for case in cases {
            assert_eq!(
                scroll_into_view(case.scroll, case.range, case.viewport_lines),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn every_block_type_renders_expected_shapes() {
        let links = HashMap::from([(1, LinkStatus::Valid), (2, LinkStatus::Invalid(404))]);
        let doc = render_settled(&full_answer(), 60, &links, DocSelection::None);
        let rendered: Vec<String> = doc.lines.iter().map(text_of).collect();
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
        assert!(joined.contains("Pipeline [1]"));
        assert!(joined.contains("● Expand"));
        assert!(joined.contains("  one call"));
        assert!(joined.contains("▼"));
        assert!(joined.contains("● Search"));
        assert!(joined.contains("▨ Figure [1]"));
        assert!(joined.contains("fetching image…"));
        assert!(joined.contains("[1] ✓ One — https://one.example (en)"));
        assert!(joined.contains("[2] ✗ 404 Two — https://two.example (en)"));
        assert!(joined.contains("→ next"));
    }

    #[test]
    fn unchecked_links_show_a_neutral_glyph() {
        let doc = render_settled(&full_answer(), 60, &HashMap::new(), DocSelection::None);
        let joined = doc
            .lines
            .iter()
            .map(|line| text_of(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("[1] · One"));
    }

    #[test]
    fn narrow_widths_never_panic() {
        for width in [0u16, 1, 5, 10] {
            let doc = render_settled(
                &full_answer(),
                width,
                &HashMap::new(),
                DocSelection::Source(0),
            );
            assert!(!doc.lines.is_empty(), "width {width}");
            for range in doc
                .block_ranges
                .iter()
                .chain(&doc.source_ranges)
                .chain(&doc.followup_ranges)
            {
                assert!(range.start <= range.end, "width {width}");
                assert!(range.end <= doc.lines.len(), "width {width}");
            }
        }
    }

    #[test]
    fn unrevealed_blocks_and_tail_sections_are_omitted() {
        let answer = full_answer();
        let partial = DocAnim {
            revealed_blocks: 1,
            ..DocAnim::settled(answer.blocks.len())
        };
        let doc = render_doc(
            &answer,
            60,
            &HashMap::new(),
            DocSelection::None,
            &partial,
            &HashMap::new(),
        );
        let joined = doc.lines.iter().map(text_of).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("Section"));
        assert!(!joined.contains("A claim with backing."));
        assert!(!joined.contains("Sources"));
        assert!(!joined.contains("→ next"));
        assert!(doc.source_ranges.is_empty());
        assert!(doc.followup_ranges.is_empty());
        for (index, range) in doc.block_ranges.iter().enumerate().skip(1) {
            assert_eq!(range.start, range.end, "block {index}");
            assert_eq!(range.end, doc.lines.len(), "block {index}");
        }
    }

    #[test]
    fn diagram_items_grow_with_the_block_fraction() {
        let answer = full_answer();
        let mut anim = DocAnim::settled(answer.blocks.len());
        anim.growth[6] = 0.5;
        let doc = render_doc(
            &answer,
            60,
            &HashMap::new(),
            DocSelection::None,
            &anim,
            &HashMap::new(),
        );
        let joined = doc.lines.iter().map(text_of).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("● Expand"));
        assert!(!joined.contains("● Search"));
    }

    #[test]
    fn image_blocks_render_caption_and_a_fixed_placeholder() {
        struct Case {
            name: &'static str,
            images: HashMap<String, ImageFetch>,
            want_first_row: &'static str,
        }
        let url = "https://one.example/figure.png";
        let cases = [
            Case {
                name: "pending fetch shows progress",
                images: HashMap::new(),
                want_first_row: "fetching image…",
            },
            Case {
                name: "failed fetch shows a note",
                images: HashMap::from([(url.to_string(), ImageFetch::Failed)]),
                want_first_row: "image unavailable",
            },
            Case {
                name: "ready image leaves blank rows for the overlay",
                images: HashMap::from([(url.to_string(), ImageFetch::Ready(vec![1]))]),
                want_first_row: "",
            },
        ];
        let answer = full_answer();
        for case in cases {
            let doc = render_doc(
                &answer,
                60,
                &HashMap::new(),
                DocSelection::None,
                &DocAnim::settled(answer.blocks.len()),
                &case.images,
            );
            assert_eq!(doc.image_slots.len(), 1, "{}", case.name);
            let slot = &doc.image_slots[0];
            assert_eq!(slot.url, url, "{}", case.name);
            assert_eq!(
                slot.range.end - slot.range.start,
                IMAGE_ROWS,
                "{}",
                case.name
            );
            assert!(
                text_of(&doc.lines[slot.range.start - 1]).starts_with("▨ Figure"),
                "{}",
                case.name
            );
            assert_eq!(
                text_of(&doc.lines[slot.range.start]).trim(),
                case.want_first_row,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn image_slots_exist_only_for_revealed_blocks() {
        let answer = full_answer();
        let partial = DocAnim {
            revealed_blocks: 1,
            ..DocAnim::settled(answer.blocks.len())
        };
        let doc = render_doc(
            &answer,
            60,
            &HashMap::new(),
            DocSelection::None,
            &partial,
            &HashMap::new(),
        );
        assert!(doc.image_slots.is_empty());
        let settled = render_settled(&answer, 60, &HashMap::new(), DocSelection::None);
        assert_eq!(settled.image_slots.len(), 1);
        assert!(settled.image_slots[0].range.end <= settled.lines.len());
    }

    #[test]
    fn visible_rows_clips_ranges_to_the_viewport() {
        struct Case {
            name: &'static str,
            range: LineRange,
            scroll: u16,
            viewport_lines: u16,
            want: Option<(u16, u16, bool)>,
        }
        let cases = [
            Case {
                name: "fully visible keeps its offset",
                range: LineRange { start: 5, end: 8 },
                scroll: 0,
                viewport_lines: 20,
                want: Some((5, 3, false)),
            },
            Case {
                name: "scrolled past the top clips from above",
                range: LineRange { start: 5, end: 8 },
                scroll: 6,
                viewport_lines: 20,
                want: Some((0, 2, true)),
            },
            Case {
                name: "overflowing the bottom clips from below",
                range: LineRange { start: 5, end: 8 },
                scroll: 0,
                viewport_lines: 6,
                want: Some((5, 1, false)),
            },
            Case {
                name: "fully above the viewport disappears",
                range: LineRange { start: 5, end: 8 },
                scroll: 10,
                viewport_lines: 20,
                want: None,
            },
            Case {
                name: "fully below the viewport disappears",
                range: LineRange { start: 5, end: 8 },
                scroll: 0,
                viewport_lines: 5,
                want: None,
            },
            Case {
                name: "zero height viewport shows nothing",
                range: LineRange { start: 0, end: 1 },
                scroll: 0,
                viewport_lines: 0,
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(
                visible_rows(case.range, case.scroll, case.viewport_lines),
                case.want,
                "{}",
                case.name
            );
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
