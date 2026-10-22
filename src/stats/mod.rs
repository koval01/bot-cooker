//! Live statistics.
//!
//! Lock-free counters for the hot path (every trapped request touches these),
//! plus a bounded per-IP table for the "hall of shame". A connection holds a
//! [`ConnGuard`] for its lifetime so active-connection count and wasted-time are
//! accounted for even if the client vanishes.

mod report;

pub use report::spawn_reporter;

use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Default)]
struct Counters {
    requests_total: AtomicU64,
    connections_total: AtomicU64,
    active: AtomicI64,
    bytes_fed: AtomicU64,
    wasted_millis: AtomicU64,
}

/// Per-IP aggregate for the leaderboard.
#[derive(Clone)]
pub struct IpRecord {
    pub requests: u64,
    pub bytes: u64,
    pub first_seen: u64, // unix secs
    pub last_seen: u64,
}

pub struct Stats {
    counters: Counters,
    ips: DashMap<IpAddr, IpRecord>,
    max_ips: usize,
    started: Instant,
    started_unix: u64,
}

impl Stats {
    pub fn new(max_ips: usize) -> Arc<Self> {
        Arc::new(Self { counters: Counters::default(), ips: DashMap::new(), max_ips, started: Instant::now(), started_unix: now_unix() })
    }

    /// Record a newly trapped request and return a guard that tracks the
    /// connection's lifetime. Dropping the guard releases the active slot and
    /// adds the elapsed time to the wasted-time total.
    pub fn open(self: &Arc<Self>, ip: IpAddr) -> ConnGuard {
        self.counters.requests_total.fetch_add(1, Ordering::Relaxed);
        self.counters.connections_total.fetch_add(1, Ordering::Relaxed);
        self.counters.active.fetch_add(1, Ordering::Relaxed);

        let now = now_unix();
        if let Some(mut rec) = self.ips.get_mut(&ip) {
            rec.requests += 1;
            rec.last_seen = now;
        } else if self.ips.len() < self.max_ips {
            self.ips.insert(ip, IpRecord { requests: 1, bytes: 0, first_seen: now, last_seen: now });
        }

        ConnGuard { stats: Arc::clone(self), ip, start: Instant::now() }
    }

    fn add_bytes(&self, ip: IpAddr, n: u64) {
        self.counters.bytes_fed.fetch_add(n, Ordering::Relaxed);
        if let Some(mut rec) = self.ips.get_mut(&ip) {
            rec.bytes += n;
        }
    }

    pub fn active(&self) -> i64 {
        self.counters.active.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            uptime_secs: self.started.elapsed().as_secs(),
            started_unix: self.started_unix,
            requests_total: self.counters.requests_total.load(Ordering::Relaxed),
            connections_total: self.counters.connections_total.load(Ordering::Relaxed),
            active: self.counters.active.load(Ordering::Relaxed).max(0) as u64,
            bytes_fed: self.counters.bytes_fed.load(Ordering::Relaxed),
            wasted_secs: self.counters.wasted_millis.load(Ordering::Relaxed) / 1000,
            unique_ips: self.ips.len() as u64,
        }
    }

    /// Top offenders by bytes fed.
    pub fn top_ips(&self, n: usize) -> Vec<(IpAddr, IpRecord)> {
        let mut v: Vec<(IpAddr, IpRecord)> = self.ips.iter().map(|e| (*e.key(), e.value().clone())).collect();
        v.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(b.1.requests.cmp(&a.1.requests)));
        v.truncate(n);
        v
    }
}

/// Lifetime tracker for one trapped connection.
pub struct ConnGuard {
    stats: Arc<Stats>,
    ip: IpAddr,
    start: Instant,
}

impl ConnGuard {
    /// Account bytes we've dripped to this client.
    pub fn fed(&self, n: u64) {
        self.stats.add_bytes(self.ip, n);
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.stats.counters.active.fetch_sub(1, Ordering::Relaxed);
        let ms = self.start.elapsed().as_millis() as u64;
        self.stats.counters.wasted_millis.fetch_add(ms, Ordering::Relaxed);
    }
}

/// A point-in-time view, used for the console report and the JSON endpoint.
#[derive(serde::Serialize)]
pub struct Snapshot {
    pub uptime_secs: u64,
    pub started_unix: u64,
    pub requests_total: u64,
    pub connections_total: u64,
    pub active: u64,
    pub bytes_fed: u64,
    pub wasted_secs: u64,
    pub unique_ips: u64,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_tracks_active_bytes_and_ips() {
        let s = Stats::new(10);
        let ip: IpAddr = "203.0.113.9".parse().unwrap();
        {
            let g = s.open(ip);
            g.fed(100);
            assert_eq!(s.active(), 1);
        }
        let _g2 = s.open(ip);
        let snap = s.snapshot();
        assert_eq!(snap.requests_total, 2);
        assert_eq!(snap.bytes_fed, 100);
        assert_eq!(snap.active, 1);
        assert_eq!(s.top_ips(5)[0].1.requests, 2);
    }

    #[test]
    fn ip_table_is_bounded() {
        let s = Stats::new(2);
        for i in 0..5u8 {
            drop(s.open(IpAddr::from([10, 0, 0, i])));
        }
        assert_eq!(s.snapshot().unique_ips, 2);
    }
}
