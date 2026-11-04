//! Minimal RFC 5545 content-line reader/writer.
//!
//! Only what a VTODO client needs: unfolding, name/params/value splitting,
//! text escaping and 75-octet line folding. Deliberately dependency free so
//! the parsing rules stay auditable and testable.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Property {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Value type from the `VALUE=` parameter, e.g. `DATE` or `DATE-TIME`.
    pub fn value_type(&self) -> Option<&str> {
        self.param("VALUE")
    }
}

/// Unfolds RFC 5545 folded lines: a CRLF followed by one space or tab
/// continues the previous logical line.
pub fn unfold(input: &str) -> Vec<String> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();

    for raw in normalized.split('\n') {
        if raw.is_empty() {
            continue;
        }
        match raw.strip_prefix([' ', '\t']) {
            Some(rest) if !out.is_empty() => {
                if let Some(last) = out.last_mut() {
                    last.push_str(rest);
                }
            }
            _ => out.push(raw.to_string()),
        }
    }

    out
}

/// Parses unfolded lines into properties, skipping `BEGIN`/`END` markers.
pub fn parse_properties(lines: &[String]) -> Vec<Property> {
    lines
        .iter()
        .filter_map(|line| parse_property(line))
        .filter(|property| {
            !property.name.eq_ignore_ascii_case("BEGIN")
                && !property.name.eq_ignore_ascii_case("END")
        })
        .collect()
}

fn parse_property(line: &str) -> Option<Property> {
    let colon = find_unquoted(line, ':')?;
    let (head, value) = line.split_at(colon);
    let value = &value[1..];

    let mut segments = split_unquoted(head, ';');
    if segments.is_empty() {
        return None;
    }
    let name = segments.remove(0).trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }

    let params = segments
        .into_iter()
        .filter_map(|segment| {
            let (key, val) = segment.split_once('=')?;
            Some((
                key.trim().to_ascii_uppercase(),
                unquote(val.trim()).to_string(),
            ))
        })
        .collect();

    Some(Property {
        name,
        params,
        value: value.to_string(),
    })
}

fn find_unquoted(input: &str, needle: char) -> Option<usize> {
    let mut quoted = false;
    for (index, ch) in input.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            c if c == needle && !quoted => return Some(index),
            _ => {}
        }
    }
    None
}

fn split_unquoted(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;

    for (index, ch) in input.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            c if c == separator && !quoted => {
                parts.push(&input[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// Escapes a TEXT value for serialization.
pub fn escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// Reverses [`escape_text`].
pub fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Appends a content line folded at 75 octets, terminated by CRLF.
pub fn fold_line(line: &str, out: &mut String) {
    const LIMIT: usize = 73;

    let mut current = String::new();
    let mut width = 0;

    for ch in line.chars() {
        let ch_width = ch.len_utf8();
        if width + ch_width > LIMIT {
            if current.is_empty() {
                continue;
            }
            out.push_str(&current);
            out.push_str("\r\n ");
            current.clear();
            width = 1;
        }
        current.push(ch);
        width += ch_width;
    }

    out.push_str(&current);
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_continuation_lines() {
        // The leading space on a continuation line is the fold marker itself,
        // so it is removed along with the CRLF.
        let input = "SUMMARY:Hello\r\n World\r\nUID:abc\r\n";
        assert_eq!(
            unfold(input),
            vec!["SUMMARY:HelloWorld".to_string(), "UID:abc".to_string()]
        );
    }

    #[test]
    fn splits_params_with_quotes() {
        let lines = vec![
            "DUE;VALUE=DATE:20260101".to_string(),
            "ATTENDEE;CN=\"Doe, John\":mailto:j@example.com".to_string(),
        ];
        let props = parse_properties(&lines);

        assert_eq!(props[0].name, "DUE");
        assert_eq!(props[0].param("VALUE"), Some("DATE"));
        assert_eq!(props[0].value, "20260101");
        assert_eq!(props[1].param("CN"), Some("Doe, John"));
        assert_eq!(props[1].value, "mailto:j@example.com");
    }

    #[test]
    fn escapes_and_unescapes_text() {
        let original = "line one\nline two; with, separators \\ backslash";
        assert_eq!(unescape_text(&escape_text(original)), original);
    }

    #[test]
    fn folds_long_lines() {
        let long = format!("DESCRIPTION:{}", "a".repeat(200));
        let mut out = String::new();
        fold_line(&long, &mut out);

        for line in out.trim_end().split("\r\n") {
            assert!(line.len() <= 74, "line too long: {}", line.len());
        }
        assert_eq!(unfold(&out).concat(), long);
    }
}
