use crate::core::answer::{Answer, Block, Emphasis};
use crate::tui::app::Pulse;
use crate::tui::theme;
use crate::tui::view::doc::DocAnim;
use ratatui::style::Style;

pub const REVEAL_STAGGER_TICKS: u64 = 2;
pub const CHART_GROW_TICKS: u64 = 6;
pub const PULSE_TICKS: u64 = 8;

pub fn revealed_blocks(started: u64, now: u64, block_count: usize) -> usize {
    let elapsed = now.saturating_sub(started);
    let shown = elapsed / REVEAL_STAGGER_TICKS + 1;
    usize::try_from(shown)
        .unwrap_or(usize::MAX)
        .min(block_count)
}

pub fn chart_fraction(started: u64, now: u64, block_index: usize) -> f64 {
    let revealed_at = block_reveal_tick(started, block_index);
    if now <= revealed_at {
        return 0.0;
    }
    let elapsed = now - revealed_at;
    if elapsed >= CHART_GROW_TICKS {
        return 1.0;
    }
    elapsed as f64 / CHART_GROW_TICKS as f64
}

pub fn pulse_style(started: u64, now: u64) -> Option<Style> {
    let elapsed = now.saturating_sub(started);
    if elapsed >= PULSE_TICKS {
        return None;
    }
    let period = PULSE_TICKS / 2;
    (elapsed % period < period / 2).then(theme::warn)
}

pub fn doc_anim(
    answer: &Answer,
    reveal_started: Option<u64>,
    pulse: Option<Pulse>,
    now: u64,
    enabled: bool,
) -> DocAnim {
    let block_count = answer.blocks.len();
    if !enabled {
        return DocAnim::settled(block_count);
    }
    let mut anim = DocAnim::settled(block_count);
    if let Some(started) = reveal_started {
        anim.revealed_blocks = revealed_blocks(started, now, block_count);
        anim.chart_growth = (0..block_count)
            .map(|index| chart_fraction(started, now, index))
            .collect();
        anim.block_overlays = answer
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| emphasis_overlay(block, started, now, index))
            .collect();
    }
    if let Some(pulse) = pulse {
        anim.source_overlay = pulse_style(pulse.started, now).map(|style| (pulse.source, style));
    }
    anim
}

fn block_reveal_tick(started: u64, block_index: usize) -> u64 {
    let offset = u64::try_from(block_index)
        .unwrap_or(u64::MAX)
        .saturating_mul(REVEAL_STAGGER_TICKS);
    started.saturating_add(offset)
}

fn emphasis_overlay(block: &Block, started: u64, now: u64, index: usize) -> Option<Style> {
    if block_emphasis(block) != Emphasis::Highlight {
        return None;
    }
    pulse_style(block_reveal_tick(started, index), now)
}

fn block_emphasis(block: &Block) -> Emphasis {
    match block {
        Block::Heading { emphasis, .. }
        | Block::Paragraph { emphasis, .. }
        | Block::List { emphasis, .. }
        | Block::Quote { emphasis, .. }
        | Block::Table { emphasis, .. }
        | Block::Chart { emphasis, .. } => *emphasis,
        Block::Unknown => Emphasis::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::answer::{Block, Emphasis};

    fn answer_with_emphasis() -> Answer {
        Answer {
            blocks: vec![
                Block::Paragraph {
                    text: "plain".to_string(),
                    source_ids: Vec::new(),
                    emphasis: Emphasis::None,
                },
                Block::Paragraph {
                    text: "key takeaway".to_string(),
                    source_ids: Vec::new(),
                    emphasis: Emphasis::Highlight,
                },
            ],
            ..Answer::default()
        }
    }

    #[test]
    fn revealed_blocks_staggers_by_tick() {
        struct Case {
            name: &'static str,
            started: u64,
            now: u64,
            block_count: usize,
            want: usize,
        }
        let cases = [
            Case {
                name: "first block shows immediately",
                started: 10,
                now: 10,
                block_count: 5,
                want: 1,
            },
            Case {
                name: "next block waits a full stagger",
                started: 10,
                now: 11,
                block_count: 5,
                want: 1,
            },
            Case {
                name: "one stagger reveals the second block",
                started: 10,
                now: 12,
                block_count: 5,
                want: 2,
            },
            Case {
                name: "reveal caps at the block count",
                started: 10,
                now: 100,
                block_count: 5,
                want: 5,
            },
            Case {
                name: "now before start is safe",
                started: 10,
                now: 5,
                block_count: 5,
                want: 1,
            },
            Case {
                name: "no blocks reveal nothing",
                started: 10,
                now: 10,
                block_count: 0,
                want: 0,
            },
        ];
        for case in cases {
            assert_eq!(
                revealed_blocks(case.started, case.now, case.block_count),
                case.want,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn chart_fraction_ramps_after_block_reveal() {
        struct Case {
            name: &'static str,
            now: u64,
            want: f64,
        }
        let cases = [
            Case {
                name: "before the block reveals",
                now: 13,
                want: 0.0,
            },
            Case {
                name: "at the reveal instant",
                now: 14,
                want: 0.0,
            },
            Case {
                name: "halfway through the growth",
                now: 17,
                want: 0.5,
            },
            Case {
                name: "growth completes",
                now: 20,
                want: 1.0,
            },
            Case {
                name: "stays complete forever",
                now: 99,
                want: 1.0,
            },
        ];
        for case in cases {
            let got = chart_fraction(10, case.now, 2);
            assert!((got - case.want).abs() < 1e-9, "{}: got {got}", case.name);
        }
    }

    #[test]
    fn pulse_style_alternates_then_stops() {
        struct Case {
            name: &'static str,
            now: u64,
            want_lit: bool,
        }
        let cases = [
            Case {
                name: "pulse starts lit",
                now: 20,
                want_lit: true,
            },
            Case {
                name: "pulse dims after two ticks",
                now: 22,
                want_lit: false,
            },
            Case {
                name: "pulse relights on the next period",
                now: 24,
                want_lit: true,
            },
            Case {
                name: "pulse ends after its window",
                now: 28,
                want_lit: false,
            },
            Case {
                name: "now before start is safe and lit",
                now: 19,
                want_lit: true,
            },
        ];
        for case in cases {
            assert_eq!(
                pulse_style(20, case.now).is_some(),
                case.want_lit,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn doc_anim_settles_when_disabled_or_unstarted() {
        struct Case {
            name: &'static str,
            reveal_started: Option<u64>,
            enabled: bool,
        }
        let cases = [
            Case {
                name: "disabled animations settle immediately",
                reveal_started: Some(10),
                enabled: false,
            },
            Case {
                name: "no reveal in flight settles",
                reveal_started: None,
                enabled: true,
            },
        ];
        let answer = answer_with_emphasis();
        for case in cases {
            let anim = doc_anim(&answer, case.reveal_started, None, 10, case.enabled);
            assert_eq!(anim, DocAnim::settled(answer.blocks.len()), "{}", case.name);
        }
    }

    #[test]
    fn doc_anim_marks_emphasized_blocks_after_their_reveal() {
        let answer = answer_with_emphasis();
        let during = doc_anim(&answer, Some(10), None, 12, true);
        assert!(during.block_overlays[1].is_some());
        assert!(during.block_overlays[0].is_none());
        let after = doc_anim(&answer, Some(10), None, 12 + PULSE_TICKS, true);
        assert!(after.block_overlays[1].is_none());
    }

    #[test]
    fn doc_anim_pulses_the_jumped_source() {
        let answer = answer_with_emphasis();
        let pulse = Pulse {
            source: 1,
            started: 10,
        };
        let fresh = doc_anim(&answer, None, Some(pulse), 10, true);
        assert_eq!(fresh.source_overlay.map(|(index, _)| index), Some(1));
        let faded = doc_anim(&answer, None, Some(pulse), 10 + PULSE_TICKS, true);
        assert_eq!(faded.source_overlay, None);
    }
}
