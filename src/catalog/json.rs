//! JSON the way a given runtime writes it. Spacing and escaping differ
//! between `json_encode`, `json.dumps` and `JSON.stringify`, and a scanner
//! that sees a PHP site answer without `\/` has found a tell.

use std::fmt::Write as _;

use serde_json::Value;

/// How one runtime encodes JSON.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// `JSON.stringify`, Jackson, System.Text.Json, nginx `escape=json`.
    Compact,
    /// PHP `json_encode`: compact, escapes `/` and non-ASCII.
    Php,
    /// Python `json.dumps`: `", "` and `": "`, escapes non-ASCII.
    Python,
}

impl Style {
    pub fn parse(name: &str) -> Option<Style> {
        match name {
            "compact" => Some(Style::Compact),
            "php" => Some(Style::Php),
            "python" => Some(Style::Python),
            _ => None,
        }
    }

    fn escapes_slash(self) -> bool {
        self == Style::Php
    }

    fn ascii_only(self) -> bool {
        matches!(self, Style::Php | Style::Python)
    }
}

/// Encode `value` in `style`, pretty-printed with `indent` spaces if given.
pub fn encode(value: &Value, style: Style, indent: Option<usize>) -> String {
    let mut out = String::new();
    write_value(&mut out, value, style, indent, 0);
    out
}

fn write_value(out: &mut String, value: &Value, style: Style, indent: Option<usize>, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(out, s, style),
        Value::Array(items) => {
            let children = items.iter().map(|v| (None, v));
            write_container(out, ('[', ']'), children, items.is_empty(), style, indent, depth);
        }
        Value::Object(map) => {
            let children = map.iter().map(|(k, v)| (Some(k.as_str()), v));
            write_container(out, ('{', '}'), children, map.is_empty(), style, indent, depth);
        }
    }
}

fn write_container<'a>(
    out: &mut String,
    (open, close): (char, char),
    children: impl Iterator<Item = (Option<&'a str>, &'a Value)>,
    empty: bool,
    style: Style,
    indent: Option<usize>,
    depth: usize,
) {
    out.push(open);
    if empty {
        out.push(close);
        return;
    }
    // Pretty output never has trailing spaces; Python's compact form does
    // put a space after `,` and `:`.
    let (item_sep, key_sep) = match (indent, style) {
        (Some(_), _) => (",", ": "),
        (None, Style::Python) => (", ", ": "),
        (None, _) => (",", ":"),
    };
    for (i, (key, child)) in children.enumerate() {
        if i > 0 {
            out.push_str(item_sep);
        }
        if let Some(n) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(n * (depth + 1)));
        }
        if let Some(k) = key {
            write_string(out, k, style);
            out.push_str(key_sep);
        }
        write_value(out, child, style, indent, depth + 1);
    }
    if let Some(n) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * depth));
    }
    out.push(close);
}

fn write_string(out: &mut String, s: &str, style: Style) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' if style.escapes_slash() => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (style.ascii_only() && !c.is_ascii()) => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn each_runtime_has_its_own_spacing_and_escaping() {
        let v = json!({"url": "https://a.example/b", "name": "Zoë", "n": [1, 2]});
        assert_eq!(encode(&v, Style::Compact, None), r#"{"url":"https://a.example/b","name":"Zoë","n":[1,2]}"#);
        assert_eq!(encode(&v, Style::Php, None), r#"{"url":"https:\/\/a.example\/b","name":"Zo\u00eb","n":[1,2]}"#);
        assert_eq!(encode(&v, Style::Python, None), r#"{"url": "https://a.example/b", "name": "Zo\u00eb", "n": [1, 2]}"#);
    }

    #[test]
    fn pretty_output_indents_without_trailing_spaces() {
        let v = json!({"a": {"b": []}, "c": [true]});
        assert_eq!(encode(&v, Style::Compact, Some(2)), "{\n  \"a\": {\n    \"b\": []\n  },\n  \"c\": [\n    true\n  ]\n}");
    }

    #[test]
    fn output_is_valid_json() {
        let v = json!({"s": "line\nquote\" back\\ ctl\u{1} emoji 🍳 /", "x": null, "f": 1.5});
        for style in [Style::Compact, Style::Php, Style::Python] {
            for indent in [None, Some(4)] {
                let text = encode(&v, style, indent);
                assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v, "{style:?} {indent:?}: {text}");
            }
        }
    }
}
