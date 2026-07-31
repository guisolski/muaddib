use crate::tui::app::{
    App, CONFIG_FIELDS, ConfigField, ConfigFieldSpec, ConfigForm, LANGUAGES, model_choices,
};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const MODAL_WIDTH: u16 = 50;
const MODAL_HEIGHT: u16 = 11;

pub fn draw(frame: &mut Frame, app: &App, form: &ConfigForm) {
    let area = centered_rect(frame.area(), MODAL_WIDTH, MODAL_HEIGHT);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(" config ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = vec![Line::default()];
    for (index, spec) in CONFIG_FIELDS.iter().enumerate() {
        lines.push(field_line(app, form, *spec, index == form.field_idx));
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        "↑↓ field · ←→ value · Enter save · Esc cancel",
        theme::dim(),
    ));
    frame.render_widget(Paragraph::new(lines), inner);
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

fn field_line(
    app: &App,
    form: &ConfigForm,
    spec: ConfigFieldSpec,
    selected: bool,
) -> Line<'static> {
    let marker = if selected { "▸ " } else { "  " };
    let label_style = if selected {
        theme::title()
    } else {
        theme::dim()
    };
    Line::from(vec![
        Span::styled(marker.to_string(), theme::citation()),
        Span::styled(format!("{:<16}", spec.label), label_style),
        Span::raw(field_value(app, form, spec.field)),
    ])
}

fn field_value(app: &App, form: &ConfigForm, field: ConfigField) -> String {
    match field {
        ConfigField::Language => LANGUAGES[form.language_idx % LANGUAGES.len()].to_string(),
        ConfigField::Engine => engine_value(app, form),
        ConfigField::Model => model_value(app, form),
        ConfigField::ValidateLinks => {
            if form.validate_links {
                "on".to_string()
            } else {
                "off".to_string()
            }
        }
        ConfigField::MaxParallel => form.max_parallel.to_string(),
    }
}

fn model_value(app: &App, form: &ConfigForm) -> String {
    app.statuses.get(form.engine_idx).map_or_else(
        || "default".to_string(),
        |status| {
            let choices = model_choices(&app.config, status.spec);
            choices[form.model_idx % choices.len()].clone()
        },
    )
}

fn engine_value(app: &App, form: &ConfigForm) -> String {
    app.statuses.get(form.engine_idx).map_or_else(
        || "none".to_string(),
        |status| {
            if status.available {
                status.spec.name.to_string()
            } else {
                format!("{} (not installed)", status.spec.name)
            }
        },
    )
}
