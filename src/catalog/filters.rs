//! Filters that encode values the way a given file format expects. They are
//! pure formatting: deciding *what* to write stays in the templates and the
//! context, deciding *how to escape it* lives here.

use minijinja::value::ValueKind;
use minijinja::{Environment, Error, ErrorKind, Value};

use crate::util::strftime;

pub fn register(env: &mut Environment<'static>) {
    env.add_filter("quote", quote);
    env.add_filter("php", php);
    env.add_filter("sql", sql);
    env.add_filter("strftime", strftime_filter);
    env.add_filter("ljust", ljust);
    env.add_filter("rjust", rjust);
    env.add_filter("human_size", human_size);
    env.add_filter("hexnum", hexnum);
    env.add_filter("base36", base36);
    env.add_filter("chunks", chunks);
    env.add_filter("choice", choice);
    env.add_filter("json", json);
}

/// `value|json(style, indent)`: JSON as a given runtime writes it (see
/// `catalog::json`); styles `compact`, `php`, `python`.
fn json(value: Value, style: Option<&str>, indent: Option<usize>) -> Result<Value, Error> {
    let style = style.unwrap_or("compact");
    let style =
        super::json::Style::parse(style).ok_or_else(|| Error::new(ErrorKind::InvalidOperation, format!("unknown json style {style}")))?;
    let tree = serde_json::to_value(&value).map_err(|e| Error::new(ErrorKind::InvalidOperation, format!("json: {e}")))?;
    Ok(Value::from_safe_string(super::json::encode(&tree, style, indent)))
}

fn text(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string(),
    }
}

/// Shell/dotenv style quoting: `bare`, `double`, `single`, or `needed`
/// (quote only when the value would otherwise break).
fn quote(value: Value, mode: String) -> Result<String, Error> {
    let v = text(&value);
    let needs = v.is_empty() || v.contains([' ', '#', '$', '"', '\'']);
    let double = || format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
    Ok(match mode.as_str() {
        "bare" if needs && !v.is_empty() => double(),
        "bare" => v,
        "double" => double(),
        "single" => format!("'{}'", v.replace('\'', "'\\''")),
        "needed" if needs => double(),
        "needed" => v,
        other => return Err(Error::new(ErrorKind::InvalidOperation, format!("unknown quote mode {other:?}"))),
    })
}

/// A PHP string literal, single- or double-quoted.
fn php(value: Value, double: Option<bool>) -> String {
    let v = text(&value);
    if double.unwrap_or(false) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$"))
    } else {
        format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// A SQL literal for `mysql`, `mssql`, or a `copy` (pg_dump COPY) field.
fn sql(value: Value, dialect: String) -> Result<String, Error> {
    let copy = dialect == "copy";
    Ok(match value.kind() {
        ValueKind::None | ValueKind::Undefined => {
            if copy {
                "\\N".into()
            } else {
                "NULL".into()
            }
        }
        ValueKind::Bool => match (copy, value.is_true()) {
            (true, true) => "t".into(),
            (true, false) => "f".into(),
            (false, b) => {
                if b {
                    "1".into()
                } else {
                    "0".into()
                }
            }
        },
        ValueKind::Number => value.to_string(),
        _ => {
            let s = text(&value);
            match dialect.as_str() {
                "copy" => s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n"),
                "mssql" => format!("N'{}'", s.replace('\'', "''")),
                "mysql" => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
                other => return Err(Error::new(ErrorKind::InvalidOperation, format!("unknown sql dialect {other:?}"))),
            }
        }
    })
}

/// `{{ ts | strftime("%d/%b/%Y", offset_min) }}`; a float timestamp carries
/// microseconds for `%f`.
fn strftime_filter(value: Value, fmt: String, offset_min: Option<i32>) -> Result<String, Error> {
    let (secs, micros) = match value.as_i64() {
        Some(i) => (i, 0),
        None => {
            let f = f64::try_from(value)?;
            (f.floor() as i64, ((f - f.floor()) * 1_000_000.0) as u32)
        }
    };
    Ok(strftime(secs, micros, offset_min.unwrap_or(0), &fmt))
}

fn ljust(value: Value, width: usize) -> String {
    format!("{:<width$}", text(&value))
}

fn rjust(value: Value, width: usize) -> String {
    format!("{:>width$}", text(&value))
}

/// Sizes as listings print them: `apache` (`21M`, `904K`) or `bytes`.
fn human_size(value: u64, style: Option<String>) -> String {
    match style.as_deref() {
        Some("apache") => apr_strfsize(value),
        Some("kib") => format!("{} KiB", value / 1024),
        _ => value.to_string(),
    }
}

/// APR's `apr_strfsize`, which Apache's directory listings use: four
/// characters, one decimal below 10 of a unit, rounded.
fn apr_strfsize(mut size: u64) -> String {
    if size < 973 {
        return format!("{size:3} ");
    }
    for unit in ['K', 'M', 'G', 'T', 'P', 'E'] {
        let mut remain = size & 1023;
        size >>= 10;
        if size >= 973 {
            continue;
        }
        if size < 9 || (size == 9 && remain < 973) {
            remain = (remain * 5 + 256) / 512;
            if remain >= 10 {
                size += 1;
                remain = 0;
            }
            return format!("{size}.{remain}{unit}");
        }
        if remain >= 512 {
            size += 1;
        }
        return format!("{size:3}{unit}");
    }
    "****".to_string()
}

fn hexnum(value: u64) -> String {
    format!("{value:x}")
}

fn base36(mut value: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(D[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A stable pick from a list: `["Log in", "Sign in"]|choice(persona_seed, "login")`.
/// Different salts make independent choices for the same seed.
fn choice(items: Vec<Value>, seed: u64, salt: Option<String>) -> Value {
    if items.is_empty() {
        return Value::from(());
    }
    let mut h: u64 = seed ^ 0x9e37_79b9_7f4a_7c15;
    for b in salt.unwrap_or_default().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h ^= h >> 29;
    items[(h % items.len() as u64) as usize].clone()
}

/// Split a string into fixed-width pieces (PEM bodies, wrapped text).
fn chunks(value: String, width: usize) -> Vec<String> {
    value.as_bytes().chunks(width.max(1)).map(|c| String::from_utf8_lossy(c).into_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    fn render(src: &str, ctx: Value) -> String {
        let mut env = Environment::new();
        register(&mut env);
        env.render_str(src, ctx).unwrap()
    }

    #[test]
    fn quoting_modes() {
        let ctx = context! { plain => "abc", spaced => "a b", dq => "x\"y" };
        assert_eq!(render("{{ plain|quote('bare') }}|{{ spaced|quote('bare') }}", ctx.clone()), "abc|\"a b\"");
        assert_eq!(render("{{ plain|quote('double') }}|{{ plain|quote('single') }}", ctx.clone()), "\"abc\"|'abc'");
        assert_eq!(render("{{ plain|quote('needed') }}|{{ dq|quote('needed') }}", ctx), "abc|\"x\\\"y\"");
    }

    #[test]
    fn php_literals() {
        let ctx = context! { v => "it's $x" };
        assert_eq!(render("{{ v|php }}", ctx.clone()), "'it\\'s $x'");
        assert_eq!(render("{{ v|php(true) }}", ctx), "\"it's \\$x\"");
    }

    #[test]
    fn sql_literals() {
        let ctx = context! { s => "O'Hara\t", n => 42, b => true, z => () };
        assert_eq!(
            render("{{ s|sql('mysql') }},{{ n|sql('mysql') }},{{ b|sql('mysql') }},{{ z|sql('mysql') }}", ctx.clone()),
            "'O\\'Hara\t',42,1,NULL"
        );
        assert_eq!(render("{{ s|sql('copy') }}|{{ b|sql('copy') }}|{{ z|sql('copy') }}", ctx.clone()), "O'Hara\\t|t|\\N");
        assert_eq!(render("{{ s|sql('mssql') }}", ctx), "N'O''Hara\t'");
    }

    #[test]
    fn unknown_modes_are_errors() {
        let mut env = Environment::new();
        register(&mut env);
        assert!(env.render_str("{{ 'x'|sql('oracle') }}", ()).is_err());
        assert!(env.render_str("{{ 'x'|quote('weird') }}", ()).is_err());
    }

    #[test]
    fn time_and_layout_helpers() {
        let ctx = context! { t => 1_792_587_909, f => 1_792_587_909.25 };
        assert_eq!(render("{{ t|strftime('%Y-%m-%d %H:%M', 60) }}", ctx.clone()), "2026-10-21 14:05");
        assert_eq!(render("{{ f|strftime('%S.%3f') }}", ctx), "09.250");
        assert_eq!(render("[{{ 'ab'|ljust(4) }}][{{ 'ab'|rjust(4) }}]", context! {}), "[ab  ][  ab]");
        assert_eq!(
            render("[{{ 22000000|human_size('apache') }}][{{ 4096|human_size('apache') }}][{{ 10|human_size }}]", context! {}),
            "[ 21M][4.0K][10]"
        );
        let apr = |n: u64| apr_strfsize(n);
        assert_eq!((apr(0), apr(972), apr(973), apr(1229)), ("  0 ".into(), "972 ".into(), "1.0K".into(), "1.2K".into()));
        assert_eq!((apr(10 * 1024), apr(1023 * 1024), apr(5 * 1024 * 1024 + 300_000)), (" 10K".into(), "1.0M".into(), "5.3M".into()));
        assert_eq!(render("{{ 255|hexnum }} {{ 71|base36 }}", context! {}), "ff 1z");
        assert_eq!(render("{{ 'abcdefg'|chunks(3)|join('/') }}", context! {}), "abc/def/g");
        let a = render("{{ ['a','b','c','d']|choice(42, 'x') }}", context! {});
        assert_eq!(a, render("{{ ['a','b','c','d']|choice(42, 'x') }}", context! {}));
        let picks: std::collections::HashSet<String> =
            (0..40).map(|s| render(&format!("{{{{ ['a','b','c','d']|choice({s}, 'x') }}}}"), context! {})).collect();
        assert!(picks.len() > 2);
    }
}
