use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Home,
    Searching,
    Results,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    ToggleHelp,
    OpenConfig,
    Back,
    Submit,
    NextMode,
    PrevMode,
    ScrollDown,
    ScrollUp,
    PageDown,
    PageUp,
    ScrollTop,
    ScrollBottom,
    FocusNext,
    FocusPrev,
    Activate,
    JumpToSource(u8),
    NewSearch,
    RefineSearch,
    HistoryPrev,
    HistoryNext,
    ClearHistory,
    ToggleFast,
    FieldNext,
    FieldPrev,
    ValueNext,
    ValuePrev,
    Confirm,
}

#[derive(Debug)]
pub struct KeyBinding {
    pub scope: Scope,
    pub code: KeyCode,
    pub mods: KeyModifiers,
    pub action: Action,
    pub help: &'static str,
}

const fn bind(
    scope: Scope,
    code: KeyCode,
    mods: KeyModifiers,
    action: Action,
    help: &'static str,
) -> KeyBinding {
    KeyBinding {
        scope,
        code,
        mods,
        action,
        help,
    }
}

pub const KEYMAP: &[KeyBinding] = &[
    bind(
        Scope::Global,
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        Action::Quit,
        "quit",
    ),
    bind(
        Scope::Global,
        KeyCode::Char('g'),
        KeyModifiers::CONTROL,
        Action::ToggleHelp,
        "toggle help",
    ),
    bind(
        Scope::Global,
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
        Action::OpenConfig,
        "open config",
    ),
    bind(
        Scope::Global,
        KeyCode::Esc,
        KeyModifiers::NONE,
        Action::Back,
        "back / cancel / close",
    ),
    bind(
        Scope::Home,
        KeyCode::Enter,
        KeyModifiers::NONE,
        Action::Submit,
        "search",
    ),
    bind(
        Scope::Home,
        KeyCode::Tab,
        KeyModifiers::NONE,
        Action::NextMode,
        "next mode",
    ),
    bind(
        Scope::Home,
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
        Action::PrevMode,
        "previous mode",
    ),
    bind(
        Scope::Home,
        KeyCode::Up,
        KeyModifiers::NONE,
        Action::HistoryPrev,
        "previous search",
    ),
    bind(
        Scope::Home,
        KeyCode::Down,
        KeyModifiers::NONE,
        Action::HistoryNext,
        "next search",
    ),
    bind(
        Scope::Home,
        KeyCode::Char('l'),
        KeyModifiers::CONTROL,
        Action::ClearHistory,
        "clear search history",
    ),
    bind(
        Scope::Home,
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
        Action::ToggleFast,
        "toggle fast mode",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('j'),
        KeyModifiers::NONE,
        Action::ScrollDown,
        "scroll down",
    ),
    bind(
        Scope::Results,
        KeyCode::Down,
        KeyModifiers::NONE,
        Action::ScrollDown,
        "scroll down",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('k'),
        KeyModifiers::NONE,
        Action::ScrollUp,
        "scroll up",
    ),
    bind(
        Scope::Results,
        KeyCode::Up,
        KeyModifiers::NONE,
        Action::ScrollUp,
        "scroll up",
    ),
    bind(
        Scope::Results,
        KeyCode::PageDown,
        KeyModifiers::NONE,
        Action::PageDown,
        "page down",
    ),
    bind(
        Scope::Results,
        KeyCode::PageUp,
        KeyModifiers::NONE,
        Action::PageUp,
        "page up",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('g'),
        KeyModifiers::NONE,
        Action::ScrollTop,
        "go to top",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('G'),
        KeyModifiers::SHIFT,
        Action::ScrollBottom,
        "go to bottom",
    ),
    bind(
        Scope::Results,
        KeyCode::Tab,
        KeyModifiers::NONE,
        Action::FocusNext,
        "next pane",
    ),
    bind(
        Scope::Results,
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
        Action::FocusPrev,
        "previous pane",
    ),
    bind(
        Scope::Results,
        KeyCode::Enter,
        KeyModifiers::NONE,
        Action::Activate,
        "open source / run follow-up",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('1'),
        KeyModifiers::NONE,
        Action::JumpToSource(1),
        "jump to source 1-9",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('n'),
        KeyModifiers::NONE,
        Action::NewSearch,
        "new search",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('/'),
        KeyModifiers::NONE,
        Action::RefineSearch,
        "refine search",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
        Action::ToggleFast,
        "toggle fast mode",
    ),
    bind(
        Scope::Results,
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        Action::Quit,
        "quit",
    ),
    bind(
        Scope::Modal,
        KeyCode::Down,
        KeyModifiers::NONE,
        Action::FieldNext,
        "next field",
    ),
    bind(
        Scope::Modal,
        KeyCode::Up,
        KeyModifiers::NONE,
        Action::FieldPrev,
        "previous field",
    ),
    bind(
        Scope::Modal,
        KeyCode::Right,
        KeyModifiers::NONE,
        Action::ValueNext,
        "next value",
    ),
    bind(
        Scope::Modal,
        KeyCode::Left,
        KeyModifiers::NONE,
        Action::ValuePrev,
        "previous value",
    ),
    bind(
        Scope::Modal,
        KeyCode::Enter,
        KeyModifiers::NONE,
        Action::Confirm,
        "save",
    ),
];

pub fn resolve(scope: Scope, key: &KeyEvent) -> Option<Action> {
    binding_in(scope, key)
        .map(|binding| binding.action)
        .or_else(|| digit_jump(scope, key))
        .or_else(|| binding_in(Scope::Global, key).map(|binding| binding.action))
}

fn digit_jump(scope: Scope, key: &KeyEvent) -> Option<Action> {
    if scope != Scope::Results || key.modifiers != KeyModifiers::NONE {
        return None;
    }
    let KeyCode::Char(character) = key.code else {
        return None;
    };
    let digit = u8::try_from(character.to_digit(10)?).ok()?;
    (1..=9)
        .contains(&digit)
        .then_some(Action::JumpToSource(digit))
}

fn binding_in(scope: Scope, key: &KeyEvent) -> Option<&'static KeyBinding> {
    KEYMAP
        .iter()
        .find(|binding| binding.scope == scope && matches_key(binding, key))
}

fn matches_key(binding: &KeyBinding, key: &KeyEvent) -> bool {
    binding.code == key.code && binding.mods == key.modifiers
}

pub fn key_label(code: KeyCode, mods: KeyModifiers) -> String {
    let base = match code {
        KeyCode::Char(letter) => letter.to_string(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab | KeyCode::BackTab => "Tab".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        other => format!("{other:?}"),
    };
    if mods.contains(KeyModifiers::CONTROL) {
        format!("Ctrl+{base}")
    } else if mods.contains(KeyModifiers::SHIFT) && !matches!(code, KeyCode::Char(_)) {
        format!("Shift+{base}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn keymap_has_no_duplicate_bindings_per_scope() {
        let mut seen = BTreeSet::new();
        for binding in KEYMAP {
            let fingerprint = format!("{:?}|{:?}|{:?}", binding.scope, binding.code, binding.mods);
            assert!(seen.insert(fingerprint), "duplicate: {binding:?}");
        }
    }

    #[test]
    fn keymap_every_binding_has_help_text() {
        for binding in KEYMAP {
            assert!(!binding.help.is_empty(), "{binding:?}");
        }
    }

    #[test]
    fn resolve_prefers_scope_bindings_and_falls_back_to_global() {
        struct Case {
            name: &'static str,
            scope: Scope,
            key: KeyEvent,
            want: Option<Action>,
        }
        let cases = [
            Case {
                name: "enter submits on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                want: Some(Action::Submit),
            },
            Case {
                name: "enter activates the selection on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                want: Some(Action::Activate),
            },
            Case {
                name: "tab cycles focus forward on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                want: Some(Action::FocusNext),
            },
            Case {
                name: "backtab cycles focus backward on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
                want: Some(Action::FocusPrev),
            },
            Case {
                name: "escape falls back to global",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                want: Some(Action::Back),
            },
            Case {
                name: "ctrl-c quits from any scope",
                scope: Scope::Searching,
                key: KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                want: Some(Action::Quit),
            },
            Case {
                name: "plain letter is not an action on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
                want: None,
            },
            Case {
                name: "ctrl-g toggles help while typing on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
                want: Some(Action::ToggleHelp),
            },
            Case {
                name: "ctrl-o opens config from results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
                want: Some(Action::OpenConfig),
            },
            Case {
                name: "plain g scrolls to top on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
                want: Some(Action::ScrollTop),
            },
            Case {
                name: "shift g goes to bottom on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT),
                want: Some(Action::ScrollBottom),
            },
        ];
        for case in cases {
            assert_eq!(resolve(case.scope, &case.key), case.want, "{}", case.name);
        }
    }

    #[test]
    fn history_and_fast_bindings_live_where_they_belong() {
        struct Case {
            name: &'static str,
            scope: Scope,
            key: KeyEvent,
            want: Option<Action>,
        }
        let cases = [
            Case {
                name: "up recalls history on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                want: Some(Action::HistoryPrev),
            },
            Case {
                name: "down walks history forward on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                want: Some(Action::HistoryNext),
            },
            Case {
                name: "up still scrolls on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                want: Some(Action::ScrollUp),
            },
            Case {
                name: "up still moves fields in a modal",
                scope: Scope::Modal,
                key: KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                want: Some(Action::FieldPrev),
            },
            Case {
                name: "ctrl-l clears history from home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
                want: Some(Action::ClearHistory),
            },
            Case {
                name: "ctrl-l is not bound outside home",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
                want: None,
            },
            Case {
                name: "ctrl-f toggles fast mode on home",
                scope: Scope::Home,
                key: KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
                want: Some(Action::ToggleFast),
            },
            Case {
                name: "ctrl-f toggles fast mode on results",
                scope: Scope::Results,
                key: KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
                want: Some(Action::ToggleFast),
            },
            Case {
                name: "ctrl-f is inert while a search runs",
                scope: Scope::Searching,
                key: KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
                want: None,
            },
        ];
        for case in cases {
            assert_eq!(resolve(case.scope, &case.key), case.want, "{}", case.name);
        }
    }

    #[test]
    fn digits_resolve_to_source_jumps_only_on_results() {
        struct Case {
            name: &'static str,
            scope: Scope,
            character: char,
            want: Option<Action>,
        }
        let cases = [
            Case {
                name: "digit one jumps to the first source",
                scope: Scope::Results,
                character: '1',
                want: Some(Action::JumpToSource(1)),
            },
            Case {
                name: "digit five jumps to the fifth source",
                scope: Scope::Results,
                character: '5',
                want: Some(Action::JumpToSource(5)),
            },
            Case {
                name: "digit nine jumps to the ninth source",
                scope: Scope::Results,
                character: '9',
                want: Some(Action::JumpToSource(9)),
            },
            Case {
                name: "digit zero is not a jump",
                scope: Scope::Results,
                character: '0',
                want: None,
            },
            Case {
                name: "digits do not jump outside results",
                scope: Scope::Home,
                character: '5',
                want: None,
            },
        ];
        for case in cases {
            let key = KeyEvent::new(KeyCode::Char(case.character), KeyModifiers::NONE);
            assert_eq!(resolve(case.scope, &key), case.want, "{}", case.name);
        }
    }

    #[test]
    fn key_labels_render_modifiers() {
        assert_eq!(
            key_label(KeyCode::Char('c'), KeyModifiers::CONTROL),
            "Ctrl+c"
        );
        assert_eq!(key_label(KeyCode::F(1), KeyModifiers::NONE), "F1");
        assert_eq!(
            key_label(KeyCode::BackTab, KeyModifiers::SHIFT),
            "Shift+Tab"
        );
        assert_eq!(key_label(KeyCode::Char('G'), KeyModifiers::SHIFT), "G");
    }
}
