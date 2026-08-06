use crate::tui::app::{
    App, ConfigField, ConfigFieldSpec, ConfigForm, LANGUAGES, api_key_label, base_url_label,
    model_choices, takes_api_key, takes_base_url,
};
use crate::tui::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

const MODAL_WIDTH: u16 = 50;
const MODAL_CHROME: u16 = 6;

pub fn modal_height(field_count: usize) -> u16 {
    MODAL_CHROME.saturating_add(u16::try_from(field_count).unwrap_or(u16::MAX))
}

pub fn draw(frame: &mut Frame, app: &App, form: &ConfigForm) {
    let fields = form.visible_fields(&app.statuses);
    let area = centered_rect(frame.area(), MODAL_WIDTH, modal_height(fields.len()));
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::citation())
        .title(" config ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines(app, form, &fields)), inner);
}

fn lines(app: &App, form: &ConfigForm, fields: &[ConfigFieldSpec]) -> Vec<Line<'static>> {
    let selected = form.field_idx % fields.len();
    let mut lines = vec![Line::default()];
    for (index, spec) in fields.iter().enumerate() {
        lines.push(field_line(app, form, *spec, index == selected));
    }
    lines.push(Line::default());
    lines.push(Line::styled(hint(form), theme::dim()));
    lines
}

fn hint(form: &ConfigForm) -> &'static str {
    if form.editing_key {
        "type the key · Enter done · Esc discard"
    } else if form.editing_url {
        "type the url · Enter done · Esc discard"
    } else {
        "↑↓ field · ←→ value · Enter save · Esc cancel"
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
        ConfigField::ApiKey => api_key_value(app, form),
        ConfigField::BaseUrl => base_url_value(app, form),
        ConfigField::ValidateLinks => toggle_value(form.validate_links),
        ConfigField::WebSearch => toggle_value(form.websearch),
        ConfigField::MaxParallel => form.max_parallel.to_string(),
    }
}

fn api_key_value(app: &App, form: &ConfigForm) -> String {
    let Some(status) = app.statuses.get(form.engine_idx) else {
        return "n/a".to_string();
    };
    let name = status.spec.name;
    let label = api_key_label(
        takes_api_key(status.spec),
        form.key_input.value(),
        status.key_from_env,
        app.vaulted.iter().any(|stored| stored == name),
    );
    if form.editing_key {
        format!("{label} ⌶")
    } else {
        label
    }
}

fn base_url_value(app: &App, form: &ConfigForm) -> String {
    let Some(status) = app.statuses.get(form.engine_idx) else {
        return "n/a".to_string();
    };
    let label = base_url_label(
        takes_base_url(status.spec),
        form.url_input.value(),
        status.endpoint.as_deref(),
    );
    if form.editing_url {
        format!("{label} ⌶")
    } else {
        label
    }
}

fn toggle_value(enabled: bool) -> String {
    if enabled { "on" } else { "off" }.to_string()
}

fn model_value(app: &App, form: &ConfigForm) -> String {
    app.statuses.get(form.engine_idx).map_or_else(
        || "default".to_string(),
        |status| {
            let choices = model_choices(&app.config, status);
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
                format!("{} ({})", status.spec.name, status.spec.missing_label)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::engines::{ENGINES, EngineStatus};
    use tui_input::Input;

    fn app_on(engine: &str, endpoint: Option<&str>) -> App {
        let statuses: Vec<EngineStatus> = ENGINES
            .iter()
            .map(|spec| EngineStatus {
                key_from_env: false,
                spec,
                available: true,
                path: None,
                endpoint: endpoint.map(ToString::to_string),
                models: spec.models.iter().map(ToString::to_string).collect(),
            })
            .collect();
        let config = Config {
            engine: engine.to_string(),
            ..Config::default()
        };
        App::new(config, statuses, None, false)
    }

    fn rendered(app: &App, form: &ConfigForm) -> String {
        let fields = form.visible_fields(&app.statuses);
        lines(app, form, &fields)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn form_for(app: &App) -> ConfigForm {
        ConfigForm::from_state(&app.config, &app.statuses)
    }

    #[test]
    fn the_modal_grows_and_shrinks_with_the_number_of_rows() {
        struct Case {
            name: &'static str,
            engine: &'static str,
        }
        let cases = [
            Case {
                name: "a cli engine",
                engine: "claude",
            },
            Case {
                name: "a keyless local server",
                engine: "ollama",
            },
            Case {
                name: "a hosted api",
                engine: "openai",
            },
        ];
        for case in cases {
            let app = app_on(case.engine, None);
            let form = form_for(&app);
            let rows = form.visible_fields(&app.statuses).len();
            assert_eq!(
                modal_height(rows),
                MODAL_CHROME + u16::try_from(rows).expect("the table is small"),
                "{}",
                case.name
            );
            assert!(
                usize::from(modal_height(rows)) > rows,
                "{}: the border and hint need room",
                case.name
            );
        }
    }

    #[test]
    fn a_cli_engine_renders_neither_api_row() {
        let app = app_on("claude", None);
        let text = rendered(&app, &form_for(&app));
        assert!(!text.contains("api key"), "{text}");
        assert!(!text.contains("base url"), "{text}");
        assert!(text.contains("model"), "{text}");
    }

    #[test]
    fn a_local_server_renders_its_endpoint_and_no_key_row() {
        let app = app_on("ollama", Some("http://localhost:11434"));
        let text = rendered(&app, &form_for(&app));
        assert!(!text.contains("api key"), "{text}");
        assert!(text.contains("base url"), "{text}");
        assert!(text.contains("http://localhost:11434"), "{text}");
    }

    #[test]
    fn the_typed_key_is_masked_while_the_base_url_is_shown_in_full() {
        let app = app_on("openai", Some("https://api.openai.com"));
        let mut form = form_for(&app);
        form.key_input = Input::new("sk-donotleak".to_string());
        form.url_input = Input::new("http://127.0.0.1:1234".to_string());
        let text = rendered(&app, &form);
        assert!(!text.contains("sk-donotleak"), "{text}");
        assert!(text.contains("•"), "{text}");
        assert!(text.contains("http://127.0.0.1:1234"), "{text}");
    }

    #[test]
    fn the_hint_line_says_what_enter_will_do() {
        struct Case {
            name: &'static str,
            editing_key: bool,
            editing_url: bool,
            want: &'static str,
        }
        let cases = [
            Case {
                name: "browsing the fields",
                editing_key: false,
                editing_url: false,
                want: "Enter save",
            },
            Case {
                name: "typing a key",
                editing_key: true,
                editing_url: false,
                want: "type the key",
            },
            Case {
                name: "typing a url",
                editing_key: false,
                editing_url: true,
                want: "type the url",
            },
        ];
        for case in cases {
            let app = app_on("openai", None);
            let mut form = form_for(&app);
            form.editing_key = case.editing_key;
            form.editing_url = case.editing_url;
            assert!(
                hint(&form).contains(case.want),
                "{}: {}",
                case.name,
                hint(&form)
            );
        }
    }

    #[test]
    fn the_cursor_marker_sits_on_the_selected_row() {
        let app = app_on("openai", None);
        let mut form = form_for(&app);
        form.field_idx = 3;
        let text = rendered(&app, &form);
        let marked: Vec<&str> = text.lines().filter(|line| line.starts_with("▸ ")).collect();
        assert_eq!(marked.len(), 1, "{text}");
        assert!(marked[0].contains("api key"), "{text}");
    }

    #[test]
    fn an_unavailable_engine_says_why_next_to_its_name() {
        let mut app = app_on("ollama", None);
        for status in &mut app.statuses {
            status.available = false;
        }
        let text = rendered(&app, &form_for(&app));
        assert!(text.contains("ollama (not running)"), "{text}");
    }
}
