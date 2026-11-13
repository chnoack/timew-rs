//! Text helpers shared by the reports: display widths, justification,
//! word wrapping, quoting and JSON string encoding.

use unicode_width::UnicodeWidthChar;

/// Number of terminal columns `c` occupies (0 for control and combining
/// characters).
pub fn char_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// Number of terminal columns `s` occupies, ignoring ANSI color codes.
pub fn display_width(s: &str) -> usize {
    strip_ansi(s).chars().map(char_width).sum()
}

/// Remove ANSI SGR sequences (`ESC ... m`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for c in s.chars() {
        if inside {
            if c == 'm' {
                inside = false;
            }
        } else if c == '\x1b' {
            inside = true;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn left_justify(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(display_width(s));
    format!("{s}{}", " ".repeat(pad))
}

pub fn right_justify(s: &str, width: usize) -> String {
    let pad = width.saturating_sub(display_width(s));
    format!("{}{s}", " ".repeat(pad))
}

/// Width of the longest space-separated word.
pub fn longest_word(s: &str) -> usize {
    s.split(' ').map(|w| w.chars().map(char_width).sum()).max().unwrap_or(0)
}

/// Width of the longest line.
pub fn longest_line(s: &str) -> usize {
    s.split('\n').map(|l| l.chars().map(char_width).sum()).max().unwrap_or(0)
}

/// Break `text` into lines of at most `width` columns. Lines break at the
/// last space that fits; a word longer than `width` is cut. Explicit
/// newlines are kept.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut offset = 0;
    while let Some(line) = extract_line(&chars, width, &mut offset) {
        lines.push(line);
    }
    lines
}

fn extract_line(text: &[char], width: usize, offset: &mut usize) -> Option<String> {
    if *offset >= text.len() {
        return None;
    }
    let slice = |from: usize, to: usize| text[from..to.max(from)].iter().collect::<String>();

    let start = *offset;
    let mut last_last;
    let mut last = start;
    let mut cursor = start;
    let mut last_space: Option<usize> = None;
    let mut line_width = 0;

    loop {
        last_last = last;
        last = cursor;
        let Some(&c) = text.get(cursor) else {
            *offset = text.len();
            return Some(slice(start, last));
        };
        cursor += 1;

        if c == '\n' {
            *offset = cursor;
            return Some(slice(start, last));
        }
        if c == ' ' {
            last_space = Some(last);
        }

        let w = char_width(c);
        if line_width + w > width {
            let before = text[last_last];
            if before != ' ' && c == ' ' {
                // "one| two": break at the space just reached.
                *offset = last + 1;
                return Some(slice(start, last));
            }
            if before == ' ' && c != ' ' && last_space.is_some() {
                // "one |two": the previous character was the space.
                *offset = last;
                return Some(slice(start, last - 1));
            }
            if before != ' ' && c != ' ' {
                if let Some(space) = last_space {
                    // "one t|wo": go back to the last space.
                    *offset = space + 1;
                    return Some(slice(start, space));
                }
                if w > width && last == start {
                    // A single character wider than the column.
                    *offset = cursor;
                    return Some(".".to_string());
                }
                // "on|e": no space to break at, cut the word.
                *offset = last;
                return Some(slice(start, last));
            }
        }
        line_width += w;
    }
}

/// Backslash-escape every `c` in `input`.
pub fn escape(input: &str, c: char) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch == c {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Put double quotes around `input` if it contains a space, a quote or an
/// operator character, the way tags are written in data files and output.
pub fn quote_if_needed(input: &str) -> String {
    const OPERATORS: &[char] = &['-', '+', '/', '(', ')', '<', '^', '!', '=', '~', '_', '%'];
    let has_quote = input.contains('"');
    if !has_quote && !input.contains(' ') && !input.contains(OPERATORS) {
        return input.to_string();
    }
    let body = if has_quote { escape(input, '"') } else { input.to_string() };
    format!("\"{body}\"")
}

/// Join with `glue`, quoting each item if needed.
pub fn join_quoted<'a>(glue: &str, items: impl IntoIterator<Item = &'a String>) -> String {
    items.into_iter().map(|s| quote_if_needed(s)).collect::<Vec<_>>().join(glue)
}

/// Encode a string for a JSON string literal (without the quotes).
pub fn json_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for c in input.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Remove one pair of matching outer quotes (`'` or `"`), if present.
pub fn dequote(input: &str) -> &str {
    let bytes = input.as_bytes();
    if bytes.len() > 1 {
        let first = bytes[0];
        if (first == b'"' || first == b'\'') && bytes[bytes.len() - 1] == first {
            return &input[1..input.len() - 1];
        }
    }
    input
}

/// Lowercase ASCII and trim.
pub fn lower_trim(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Truthy configuration values.
pub fn is_true(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "y" | "yes" | "on")
}

/// Unique-prefix completion: an exact match wins, otherwise all items that
/// start with `partial`.
pub fn auto_complete<'a>(partial: &str, list: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut matches = Vec::new();
    if partial.is_empty() {
        return matches;
    }
    for item in list {
        if item == partial {
            return vec![item];
        }
        if item.starts_with(partial) {
            matches.push(item);
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap_text("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap_text("one two three", 5), vec!["one", "two", "three"]);
        assert_eq!(wrap_text("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap_text("", 3), Vec::<String>::new());
        assert_eq!(wrap_text("a\nb", 10), vec!["a", "b"]);
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_if_needed("foo"), "foo");
        assert_eq!(quote_if_needed("foo bar"), "\"foo bar\"");
        assert_eq!(quote_if_needed("client-a"), "\"client-a\"");
        assert_eq!(quote_if_needed("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn json() {
        assert_eq!(json_encode("a/b \"c\"\n"), "a\\/b \\\"c\\\"\\n");
    }

    #[test]
    fn completion() {
        let list = ["start", "stop", "summary"];
        assert_eq!(auto_complete("sta", list), vec!["start"]);
        assert_eq!(auto_complete("s", list).len(), 3);
        assert_eq!(auto_complete("stop", list), vec!["stop"]);
    }
}
