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

#[cfg(test)]
mod tests {
    use super::*;

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
}
