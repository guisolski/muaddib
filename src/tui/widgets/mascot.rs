use crate::tui::app::{App, Screen};
use crate::tui::theme;
use ratatui::text::{Line, Span};

pub const SCENE_WIDTH: usize = 14;
pub const IDLE_SLOWDOWN: u64 = 4;
pub const SAD_TICKS: u64 = 30;
pub const CELEBRATE_TICKS: u64 = 12;
pub const WORM_PERIOD: u64 = 600;
pub const WORM_DELAY: u64 = 570;
pub const WORM_STRIDE: u64 = 3;
const HOP_PERIOD: u64 = 4;
const SPRITE_SPAN: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MascotState {
    Sleeping,
    Sad,
    Hopping,
    Celebrating,
    Hidden,
}

#[derive(Clone, Copy)]
struct SpriteFrame {
    body: &'static str,
    deco: &'static str,
}

const EARS: &str = "   \\ /        ";
const FEET: &str = "  (_)(_)      ";
const AWAKE: SpriteFrame = SpriteFrame {
    body: "  (o.o)~-,",
    deco: "    ",
};
const SAD_FRAME: SpriteFrame = SpriteFrame {
    body: "  (u.u)~-,",
    deco: " …  ",
};
const HAPPY: SpriteFrame = SpriteFrame {
    body: "  (^.^)~-,",
    deco: " !  ",
};
const SLEEPING: [SpriteFrame; 8] = [
    SpriteFrame {
        body: "  (-.-)~-,",
        deco: "    ",
    },
    SpriteFrame {
        body: "  (-.-)~-,",
        deco: " ᶻ  ",
    },
    SpriteFrame {
        body: "  (-.-)-~,",
        deco: " ᶻᶻ ",
    },
    SpriteFrame {
        body: "  (-.-)~-,",
        deco: " ᶻᶻ ",
    },
    SpriteFrame {
        body: "  (-.-)-~,",
        deco: " ᶻ  ",
    },
    SpriteFrame {
        body: "  (-.-)~-,",
        deco: "    ",
    },
    SpriteFrame {
        body: "  (o.o)-~,",
        deco: "    ",
    },
    SpriteFrame {
        body: "  (-.-)~-,",
        deco: "    ",
    },
];
const DUNE_WAVES: &str = "﹏﹏﹏";
const DUNE_PEAK: &str = "▲";
const WORM: [&str; 8] = [
    "◉﹏﹏﹏▲﹏﹏  ",
    "〰◉﹏﹏▲﹏﹏  ",
    "〰〰◉﹏▲﹏﹏  ",
    " 〰〰◉▲﹏﹏   ",
    "﹏ 〰〰◉﹏﹏  ",
    "﹏﹏ 〰〰◉    ",
    "﹏﹏﹏▲〰〰◉  ",
    "﹏﹏﹏▲﹏〰◉  ",
];
const HOP_EARS: &str = "\\ /";
const HOP_BODY: &str = "(o.o)~-,";
const HOP_BODY_SWISH: &str = "(o.o)-~,";
const HOP_FEET: &str = "(_)(_)";
const HOP_FEET_AIR: &str = "/  \\";

pub fn mascot_state(app: &App) -> MascotState {
    match app.screen {
        Screen::Searching => MascotState::Hopping,
        Screen::Home if recently(app.search.failed_at, app.tick, SAD_TICKS) => MascotState::Sad,
        Screen::Home => MascotState::Sleeping,
        Screen::Results if recently(app.search.reveal_started, app.tick, CELEBRATE_TICKS) => {
            MascotState::Celebrating
        }
        Screen::Results | Screen::Tree => MascotState::Hidden,
    }
}

fn recently(started: Option<u64>, now: u64, window: u64) -> bool {
    started.is_some_and(|tick| now.saturating_sub(tick) < window)
}

pub fn home_scene(state: MascotState, tick: u64, animations: bool) -> [Line<'static>; 4] {
    let frame = scene_frame(state, tick, animations);
    [
        Line::styled(EARS, theme::mascot()),
        Line::from(vec![
            Span::styled(frame.body, theme::mascot()),
            Span::styled(frame.deco, theme::dim()),
        ]),
        Line::styled(FEET, theme::mascot()),
        ground_line(state, tick, animations),
    ]
}

fn scene_frame(state: MascotState, tick: u64, animations: bool) -> SpriteFrame {
    match state {
        MascotState::Sad => SAD_FRAME,
        MascotState::Celebrating => HAPPY,
        MascotState::Sleeping if animations => {
            SLEEPING[((tick / IDLE_SLOWDOWN) % SLEEPING.len() as u64) as usize]
        }
        MascotState::Sleeping | MascotState::Hopping | MascotState::Hidden => AWAKE,
    }
}

fn ground_line(state: MascotState, tick: u64, animations: bool) -> Line<'static> {
    if animations
        && state != MascotState::Sad
        && let Some(worm) = worm_frame(tick)
    {
        return Line::styled(worm, theme::warn());
    }
    Line::from(vec![
        Span::raw(" "),
        Span::styled(DUNE_WAVES, theme::warn()),
        Span::styled(DUNE_PEAK, theme::title()),
        Span::styled(DUNE_WAVES, theme::warn()),
    ])
}

fn worm_frame(tick: u64) -> Option<&'static str> {
    let phase = (tick % WORM_PERIOD).checked_sub(WORM_DELAY)?;
    WORM.get((phase / WORM_STRIDE) as usize).copied()
}

pub fn hop_lines(tick: u64, width: usize, animations: bool) -> [Line<'static>; 5] {
    let track = width.saturating_sub(SPRITE_SPAN).max(1) as u64;
    let offset = if animations {
        ((tick * 2) % track) as usize
    } else {
        0
    };
    let airborne = animations && tick % HOP_PERIOD >= HOP_PERIOD / 2;
    let body = if airborne { HOP_BODY_SWISH } else { HOP_BODY };
    if airborne {
        [
            sprite_row(offset + 1, HOP_EARS, width),
            sprite_row(offset, body, width),
            air_feet_row(offset, width),
            blank_row(width),
            hop_ground(width),
        ]
    } else {
        [
            blank_row(width),
            sprite_row(offset + 1, HOP_EARS, width),
            sprite_row(offset, body, width),
            sprite_row(offset, HOP_FEET, width),
            hop_ground(width),
        ]
    }
}

pub fn celebrate_span(reveal_started: u64, now: u64) -> Option<Span<'static>> {
    let elapsed = now.saturating_sub(reveal_started);
    (elapsed < CELEBRATE_TICKS).then(|| {
        let face = if elapsed % 4 < 2 {
            "(^.^)! "
        } else {
            "(^-^)! "
        };
        Span::styled(face, theme::mascot())
    })
}

fn sprite_row(indent: usize, art: &'static str, width: usize) -> Line<'static> {
    Line::styled(
        padded(&format!("{}{art}", " ".repeat(indent)), width),
        theme::mascot(),
    )
}

fn air_feet_row(offset: usize, width: usize) -> Line<'static> {
    if offset >= 3 {
        let dust = format!("{}˖∘", " ".repeat(offset - 3));
        let feet = padded(
            &format!(" {HOP_FEET_AIR}"),
            width.saturating_sub(offset - 1),
        );
        Line::from(vec![
            Span::styled(dust, theme::dim()),
            Span::styled(feet, theme::mascot()),
        ])
    } else {
        sprite_row(offset + 1, HOP_FEET_AIR, width)
    }
}

fn blank_row(width: usize) -> Line<'static> {
    Line::raw(" ".repeat(width))
}

fn hop_ground(width: usize) -> Line<'static> {
    let mut waves = "﹏".repeat(width / 2);
    if width % 2 == 1 {
        waves.push(' ');
    }
    Line::styled(waves, theme::warn())
}

fn padded(text: &str, width: usize) -> String {
    let clipped: String = text.chars().take(width).collect();
    format!("{clipped:<width$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Config;
    use crate::engines::{ENGINES, EngineStatus};
    use std::path::PathBuf;

    fn line_text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn test_app() -> App {
        let statuses: Vec<EngineStatus> = ENGINES
            .iter()
            .map(|spec| EngineStatus {
                spec,
                available: true,
                path: Some(PathBuf::from("/fake/bin")),
            })
            .collect();
        App::new(Config::default(), statuses, None, false)
    }

    #[test]
    fn every_scene_line_spans_the_full_scene_width() {
        let states = [
            MascotState::Sleeping,
            MascotState::Sad,
            MascotState::Hopping,
            MascotState::Celebrating,
            MascotState::Hidden,
        ];
        for state in states {
            for animations in [true, false] {
                for tick in (0..40).chain(WORM_DELAY..WORM_DELAY + 30) {
                    for (row, line) in home_scene(state, tick, animations).iter().enumerate() {
                        assert_eq!(
                            line.width(),
                            SCENE_WIDTH,
                            "{state:?} animations {animations} tick {tick} row {row}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn hop_lines_fill_the_requested_width_exactly() {
        for width in [20usize, 45, 70] {
            for animations in [true, false] {
                for tick in 0..40 {
                    for (row, line) in hop_lines(tick, width, animations).iter().enumerate() {
                        assert_eq!(
                            line.width(),
                            width,
                            "width {width} animations {animations} tick {tick} row {row}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn sleeping_breath_slows_and_wraps() {
        struct Case {
            name: &'static str,
            tick_a: u64,
            tick_b: u64,
            want_same: bool,
        }
        let cases = [
            Case {
                name: "consecutive ticks share a frame",
                tick_a: 0,
                tick_b: 3,
                want_same: true,
            },
            Case {
                name: "the next slot advances the frame",
                tick_a: 0,
                tick_b: 4,
                want_same: false,
            },
            Case {
                name: "the cycle wraps",
                tick_a: 0,
                tick_b: 32,
                want_same: true,
            },
        ];
        for case in cases {
            let a = line_text(&home_scene(MascotState::Sleeping, case.tick_a, true)[1]);
            let b = line_text(&home_scene(MascotState::Sleeping, case.tick_b, true)[1]);
            assert_eq!(a == b, case.want_same, "{}", case.name);
        }
    }

    #[test]
    fn the_worm_crosses_on_a_fixed_schedule() {
        struct Case {
            name: &'static str,
            tick: u64,
            state: MascotState,
            animations: bool,
            want_worm: bool,
        }
        let cases = [
            Case {
                name: "quiet before the pass",
                tick: WORM_DELAY - 1,
                state: MascotState::Sleeping,
                animations: true,
                want_worm: false,
            },
            Case {
                name: "the pass begins",
                tick: WORM_DELAY,
                state: MascotState::Sleeping,
                animations: true,
                want_worm: true,
            },
            Case {
                name: "the pass ends",
                tick: WORM_DELAY + WORM_STRIDE * 8,
                state: MascotState::Sleeping,
                animations: true,
                want_worm: false,
            },
            Case {
                name: "the schedule repeats",
                tick: WORM_PERIOD + WORM_DELAY,
                state: MascotState::Sleeping,
                animations: true,
                want_worm: true,
            },
            Case {
                name: "static scenes stay calm",
                tick: WORM_DELAY,
                state: MascotState::Sleeping,
                animations: false,
                want_worm: false,
            },
            Case {
                name: "grief keeps the dunes still",
                tick: WORM_DELAY,
                state: MascotState::Sad,
                animations: true,
                want_worm: false,
            },
        ];
        for case in cases {
            let ground = line_text(&home_scene(case.state, case.tick, case.animations)[3]);
            assert_eq!(ground.contains('◉'), case.want_worm, "{}", case.name);
        }
    }

    #[test]
    fn the_hop_advances_wraps_and_alternates_air_and_ground() {
        let grounded = hop_lines(0, 40, true);
        assert_eq!(line_text(&grounded[0]).trim(), "");
        assert!(line_text(&grounded[1]).contains("\\ /"));
        assert_eq!(line_text(&grounded[3]).find("(_)(_)"), Some(0));
        let airborne = hop_lines(2, 40, true);
        assert!(line_text(&airborne[0]).contains("\\ /"));
        assert!(line_text(&airborne[2]).contains("/  \\"));
        let later = hop_lines(8, 40, true);
        assert_eq!(line_text(&later[3]).find("(_)(_)"), Some(16));
        let wrapped = hop_lines(16, 40, true);
        assert_eq!(line_text(&wrapped[3]).find("(_)(_)"), Some(2));
    }

    #[test]
    fn the_mascot_state_follows_the_app() {
        struct Case {
            name: &'static str,
            screen: Screen,
            failed_at: Option<u64>,
            reveal_started: Option<u64>,
            tick: u64,
            want: MascotState,
        }
        let cases = [
            Case {
                name: "home starts sleeping",
                screen: Screen::Home,
                failed_at: None,
                reveal_started: None,
                tick: 0,
                want: MascotState::Sleeping,
            },
            Case {
                name: "a fresh failure saddens",
                screen: Screen::Home,
                failed_at: Some(10),
                reveal_started: None,
                tick: 20,
                want: MascotState::Sad,
            },
            Case {
                name: "grief fades back to sleep",
                screen: Screen::Home,
                failed_at: Some(10),
                reveal_started: None,
                tick: 10 + SAD_TICKS,
                want: MascotState::Sleeping,
            },
            Case {
                name: "searching hops",
                screen: Screen::Searching,
                failed_at: None,
                reveal_started: None,
                tick: 5,
                want: MascotState::Hopping,
            },
            Case {
                name: "a fresh answer celebrates",
                screen: Screen::Results,
                failed_at: None,
                reveal_started: Some(100),
                tick: 105,
                want: MascotState::Celebrating,
            },
            Case {
                name: "results settle to hidden",
                screen: Screen::Results,
                failed_at: None,
                reveal_started: Some(100),
                tick: 100 + CELEBRATE_TICKS,
                want: MascotState::Hidden,
            },
        ];
        for case in cases {
            let mut app = test_app();
            app.screen = case.screen;
            app.search.failed_at = case.failed_at;
            app.search.reveal_started = case.reveal_started;
            app.tick = case.tick;
            assert_eq!(mascot_state(&app), case.want, "{}", case.name);
        }
    }

    #[test]
    fn disabled_animations_pin_a_single_static_frame() {
        let reference = home_scene(MascotState::Sleeping, 0, false);
        for tick in [7, 300, WORM_DELAY, 599] {
            assert_eq!(
                home_scene(MascotState::Sleeping, tick, false),
                reference,
                "tick {tick}"
            );
        }
    }

    #[test]
    fn the_celebration_expires() {
        assert!(celebrate_span(100, 100).is_some());
        assert!(celebrate_span(100, 100 + CELEBRATE_TICKS - 1).is_some());
        assert!(celebrate_span(100, 100 + CELEBRATE_TICKS).is_none());
    }
}
