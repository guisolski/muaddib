use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Cyan;
pub const OK: Color = Color::Green;
pub const ERR: Color = Color::Red;
pub const WARN: Color = Color::Yellow;
pub const DIM: Color = Color::DarkGray;

pub fn title() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn heading() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn mascot() -> Style {
    Style::new().fg(ACCENT)
}

pub fn dim() -> Style {
    Style::new().fg(DIM)
}

pub fn citation() -> Style {
    Style::new().fg(ACCENT)
}

pub fn ok() -> Style {
    Style::new().fg(OK)
}

pub fn err() -> Style {
    Style::new().fg(ERR)
}

pub fn warn() -> Style {
    Style::new().fg(WARN)
}

pub fn selected() -> Style {
    Style::new().add_modifier(Modifier::REVERSED)
}

pub fn quote() -> Style {
    Style::new().fg(DIM).add_modifier(Modifier::ITALIC)
}
