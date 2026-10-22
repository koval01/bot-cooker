//! Objects templates call into for values: `secret.*` (instance-wide secrets
//! that rotate with config epochs), `tok.*` (stable per page and log epoch)
//! and `rand.*` (fresh per request, only for headers such as cookies).

use std::sync::Arc;

use minijinja::value::{Object, Value};
use minijinja::{Error, ErrorKind, State};
use rand::{Rng, RngExt};

use crate::util::{self, alphabet, keyed_rng};

fn bad(msg: String) -> Error {
    Error::new(ErrorKind::InvalidOperation, msg)
}

fn arg_str(args: &[Value], i: usize) -> Result<String, Error> {
    args.get(i).and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| bad(format!("argument {i} must be a string")))
}

fn arg_len(args: &[Value], i: usize) -> Result<usize, Error> {
    args.get(i).and_then(Value::as_i64).map(|n| n.clamp(0, 4096) as usize).ok_or_else(|| bad(format!("argument {i} must be a length")))
}

/// Derivation tag of `get`, shared with `World::secret`.
pub const DEFAULT: &[u8] = b"default";

/// Keyed, deterministic tokens: `obj.<alphabet>(label, len)`, `obj.uuid(label)`,
/// and for secrets `obj.get(label)` in the persona's default secret style.
#[derive(Debug)]
pub struct Keyed {
    key: [u8; 32],
    default_alphabet: &'static [u8],
    default_len: usize,
}

impl Keyed {
    pub fn value(key: [u8; 32], default_alphabet: &'static [u8], default_len: usize) -> Value {
        Value::from_object(Keyed { key, default_alphabet, default_len })
    }

    fn rng(&self, label: &str, extra: &[u8]) -> rand_chacha::ChaCha8Rng {
        keyed_rng(&self.key, &[label.as_bytes(), extra])
    }
}

impl Object for Keyed {
    fn call_method(self: &Arc<Self>, _state: &mut State<'_, '_>, method: &str, args: &[Value]) -> Result<Value, Error> {
        let label = arg_str(args, 0)?;
        match method {
            "get" => Ok(Value::from(util::token(&mut self.rng(&label, DEFAULT), self.default_alphabet, self.default_len))),
            "uuid" => Ok(Value::from(util::uuid(&mut self.rng(&label, b"uuid")))),
            "int" => {
                let (lo, hi) = (args.get(1).and_then(Value::as_i64).unwrap_or(0), args.get(2).and_then(Value::as_i64).unwrap_or(100));
                Ok(Value::from(self.rng(&label, b"int").random_range(lo..hi.max(lo + 1))))
            }
            m => {
                let a = alphabet(m).ok_or_else(|| bad(format!("unknown token method {m:?}")))?;
                let n = arg_len(args, 1)?;
                Ok(Value::from(util::token(&mut self.rng(&label, m.as_bytes()), a, n)))
            }
        }
    }
}

/// Per-request randomness: `rand.<alphabet>(len)`, `rand.uuid()`,
/// `rand.runtime()` (a plausible request duration in seconds).
#[derive(Debug, Default)]
pub struct Fresh;

impl Fresh {
    pub fn value() -> Value {
        Value::from_object(Fresh)
    }
}

impl Object for Fresh {
    fn call_method(self: &Arc<Self>, _state: &mut State<'_, '_>, method: &str, args: &[Value]) -> Result<Value, Error> {
        let mut rng = rand::rng();
        match method {
            "uuid" => Ok(Value::from(util::uuid(&mut rng))),
            "runtime" => Ok(Value::from(format!("{:.6}", rng.random_range(0.004..0.4)))),
            m => {
                let a = alphabet(m).ok_or_else(|| bad(format!("unknown rand method {m:?}")))?;
                let n = arg_len(args, 0)?;
                let mut out = String::with_capacity(n);
                for _ in 0..n {
                    out.push(a[(rng.next_u32() as usize) % a.len()] as char);
                }
                Ok(Value::from(out))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::{Environment, context};

    #[test]
    fn keyed_tokens_are_stable_and_shaped() {
        let env = Environment::new();
        let k = Keyed::value([7; 32], util::ALNUM, 12);
        let ctx = context! { s => k };
        let a = env.render_str("{{ s.get('db') }}|{{ s.hex('app', 8) }}|{{ s.uuid('x') }}", ctx.clone()).unwrap();
        let b = env.render_str("{{ s.get('db') }}|{{ s.hex('app', 8) }}|{{ s.uuid('x') }}", ctx).unwrap();
        assert_eq!(a, b);
        let parts: Vec<&str> = a.split('|').collect();
        assert_eq!(parts[0].len(), 12);
        assert!(parts[1].chars().all(|c| c.is_ascii_hexdigit()) && parts[1].len() == 8);
        assert_eq!(parts[2].len(), 36);
    }

    #[test]
    fn fresh_values_change() {
        let env = Environment::new();
        let ctx = context! { rand => Fresh::value() };
        let a = env.render_str("{{ rand.alnum(24) }}", ctx.clone()).unwrap();
        let b = env.render_str("{{ rand.alnum(24) }}", ctx).unwrap();
        assert_eq!(a.len(), 24);
        assert_ne!(a, b);
    }

    #[test]
    fn unknown_methods_fail() {
        let env = Environment::new();
        assert!(env.render_str("{{ r.oops(3) }}", context! { r => Fresh::value() }).is_err());
    }
}
