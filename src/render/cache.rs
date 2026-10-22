//! Bodies of expensive static files, kept for one persona era. A real server
//! reads the same bytes from disk on every request, so serving a cached body
//! changes nothing a client can observe; headers (request ids, cookies) are
//! built per request and never cached.

use std::collections::HashMap;
use std::sync::Mutex;

use super::doc::Doc;

/// Upper bound on cached body bytes.
const BUDGET: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub struct DocCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    docs: HashMap<String, (Doc, u64)>,
    bytes: usize,
    tick: u64,
}

impl DocCache {
    /// The cached doc for `key`, or render, keep and return it.
    pub fn get_or_render(&self, key: String, render: impl FnOnce() -> Doc) -> Doc {
        if let Ok(mut inner) = self.inner.lock() {
            inner.tick += 1;
            let tick = inner.tick;
            if let Some((doc, used)) = inner.docs.get_mut(&key) {
                *used = tick;
                return doc.clone();
            }
        }
        // Render outside the lock: two concurrent misses both render, which
        // is cheaper than making every request wait on one.
        let doc = render();
        if let Ok(mut inner) = self.inner.lock() {
            let size = size(&doc);
            if size <= BUDGET / 4 {
                let tick = inner.tick;
                if let Some((old, _)) = inner.docs.insert(key, (doc.clone(), tick)) {
                    inner.bytes -= self::size(&old);
                }
                inner.bytes += size;
                inner.evict();
            }
        }
        doc
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().map(|i| i.docs.len()).unwrap_or(0)
    }
}

impl Inner {
    /// Drop the least recently used docs until a quarter of the budget is free.
    fn evict(&mut self) {
        if self.bytes <= BUDGET {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self.docs.iter().map(|(k, (_, used))| (*used, k.clone())).collect();
        by_age.sort_unstable();
        for (_, key) in by_age {
            if self.bytes <= BUDGET * 3 / 4 {
                break;
            }
            if let Some((doc, _)) = self.docs.remove(&key) {
                self.bytes -= size(&doc);
            }
        }
    }
}

fn size(doc: &Doc) -> usize {
    doc.head.len() + doc.tail.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::doc::Origin;

    fn doc(n: usize) -> Doc {
        Doc::new(Origin::Server, None, vec![b'x'; n])
    }

    #[test]
    fn renders_once_per_key() {
        let cache = DocCache::default();
        let mut calls = 0;
        for _ in 0..3 {
            let d = cache.get_or_render("a".into(), || {
                calls += 1;
                doc(10)
            });
            assert_eq!(d.head.len(), 10);
        }
        assert_eq!(calls, 1);
    }

    #[test]
    fn stays_within_budget() {
        let cache = DocCache::default();
        for i in 0..40 {
            cache.get_or_render(format!("k{i}"), || doc(1024 * 1024));
        }
        let bytes = cache.inner.lock().unwrap().bytes;
        assert!(bytes <= BUDGET, "{bytes}");
        assert!(cache.len() < 40);
        // The most recent one survives eviction.
        let mut rendered = false;
        cache.get_or_render("k39".into(), || {
            rendered = true;
            doc(1)
        });
        assert!(!rendered);
    }
}
