//! Console "kitchen report" and human-readable formatting.

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Duration;

use super::Stats;

pub fn human_bytes(n: u64) -> String {
    const U: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} {}", U[0]) } else { format!("{v:.2} {}", U[i]) }
}

pub fn human_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d {h}h {m}m")
    } else if h > 0 {
        format!("{h}h {m}m {s}s")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Spawn the periodic reporter: a colour box on a terminal, one log line
/// otherwise (services, `docker logs`, log files).
pub fn spawn_reporter(stats: Arc<Stats>, interval: Duration) {
    if interval.is_zero() {
        return;
    }
    let terminal = std::io::stdout().is_terminal();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if terminal {
                print_report(&stats);
            } else {
                tracing::info!("{}", summary_line(&stats));
            }
        }
    });
}

fn summary_line(stats: &Stats) -> String {
    let s = stats.snapshot();
    format!(
        "report: active {} | conns {} | requests {} | unique IPs {} | fed {} | bot-time {} | up {}",
        s.active,
        s.connections_total,
        s.requests_total,
        s.unique_ips,
        human_bytes(s.bytes_fed),
        human_duration(s.wasted_secs),
        human_duration(s.uptime_secs)
    )
}

const C_RESET: &str = "\x1b[0m";
const C_DIM: &str = "\x1b[2m";
const C_CYAN: &str = "\x1b[36m";
const C_GREEN: &str = "\x1b[32m";
const C_YELLOW: &str = "\x1b[33m";
const C_BOLD: &str = "\x1b[1m";

fn print_report(stats: &Arc<Stats>) {
    // Bind locals so format-string captures don't depend on const capture.
    let rst = C_RESET;
    let dim = C_DIM;
    let cyan = C_CYAN;
    let green = C_GREEN;
    let yellow = C_YELLOW;
    let bold = C_BOLD;

    let s = stats.snapshot();
    let top = stats.top_ips(5);

    println!("\n{bold}{cyan}╭─ bot-cooker ─ kitchen report ─ uptime {up}{rst}", up = human_duration(s.uptime_secs));
    println!("{cyan}│{rst} active now    {green}{:>12}{rst}   total conns {:>12}", s.active, s.connections_total);
    println!("{cyan}│{rst} requests      {:>12}   unique IPs  {:>12}", s.requests_total, s.unique_ips);
    println!(
        "{cyan}│{rst} garbage fed   {yellow}{gb:>12}{rst}   bot-time    {yellow}{bt:>12}{rst}",
        gb = human_bytes(s.bytes_fed),
        bt = human_duration(s.wasted_secs)
    );
    if !top.is_empty() {
        println!("{cyan}│{rst} {dim}top guests:{rst}");
        for (ip, rec) in top {
            println!(
                "{cyan}│{rst}   {ips:<40} {gb:>10}  {reqs} reqs",
                ips = ip.to_string(),
                gb = human_bytes(rec.bytes),
                reqs = rec.requests
            );
        }
    }
    println!("{cyan}╰─────────────────────────────────────────────{rst}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_line_is_plain_text() {
        let line = summary_line(&Stats::new(10));
        assert!(line.starts_with("report: active 0"));
        assert!(!line.contains('\x1b'));
    }

    #[test]
    fn human_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.50 KiB");
        assert_eq!(human_duration(59), "59s");
        assert_eq!(human_duration(3_725), "1h 2m 5s");
        assert_eq!(human_duration(90_000), "1d 1h 0m");
    }
}
