pub const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn frame(tick: u64) -> &'static str {
    FRAMES[(tick % FRAMES.len() as u64) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_cycles_through_the_table() {
        assert_eq!(frame(0), FRAMES[0]);
        assert_eq!(frame(9), FRAMES[9]);
        assert_eq!(frame(10), FRAMES[0]);
        assert_eq!(frame(25), FRAMES[5]);
    }
}
