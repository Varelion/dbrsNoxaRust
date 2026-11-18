//! Logging: what the bot did, what went wrong, and how it is holding up.
//!
//! ```text
//!   tracing ─► stdout ─► journald        journalctl -u noxadbrs [-p warning] [-g 123456]
//!     ├─ interaction  one span each: id, kind, command, user, channel; outcome and milliseconds
//!     ├─ audit        every #mod-log line, kept here too, so a change Discord refused is not lost
//!     ├─ panic        message, location, backtrace, inside the span that panicked
//!     ├─ gateway      connects, resumes, reconnects, closes, lost events
//!     └─ health       hourly: memory, uptime, interactions ok / failed / panicked, reconnects
//! ```
//!
//! RUST_LOG picks the detail, e.g. `RUST_LOG=info,noxadbrs=debug` adds every reply's text.

use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const DEFAULT_FILTER: &str = "info";
const HEALTH_EVERY: Duration = Duration::from_secs(60 * 60);
const KIB_PER_MIB: u64 = 1024;
const SECONDS_PER_HOUR: u64 = 3600;

pub(crate) fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    // Colour only on a terminal; journald gets plain text.
    let ansi = std::io::stdout().is_terminal();
    tracing_subscriber::fmt().with_env_filter(filter).with_ansi(ansi).with_target(true).init();

    // A panic anywhere is logged with where it happened and how it got there, inside the span it hit.
    std::panic::set_hook(Box::new(|panic| {
        let payload = panic.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let location = panic.location().map(|at| format!("{}:{}", at.file(), at.line())).unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture();
        error!(target: "panic", %location, %message, %backtrace, "panicked");
    }));
}

/// What the health line counts.
#[derive(Clone, Copy)]
pub(crate) enum Counter {
    Done,
    Failed,
    Panicked,
    Reconnect,
    LostEvent,
}

pub(crate) struct Stats {
    started: Instant,
    done: AtomicU64,
    failed: AtomicU64,
    panicked: AtomicU64,
    reconnects: AtomicU64,
    lost_events: AtomicU64,
}

impl Stats {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            done: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            panicked: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            lost_events: AtomicU64::new(0),
        }
    }

    pub(crate) fn bump(&self, counter: Counter) {
        let cell = match counter {
            Counter::Done => &self.done,
            Counter::Failed => &self.failed,
            Counter::Panicked => &self.panicked,
            Counter::Reconnect => &self.reconnects,
            Counter::LostEvent => &self.lost_events,
        };
        cell.fetch_add(1, Ordering::Relaxed);
    }

    fn log(&self) {
        info!(
            target: "health",
            rss_mib = rss_kib().map(|kib| kib / KIB_PER_MIB),
            uptime_hours = self.started.elapsed().as_secs() / SECONDS_PER_HOUR,
            done = self.done.load(Ordering::Relaxed),
            failed = self.failed.load(Ordering::Relaxed),
            panicked = self.panicked.load(Ordering::Relaxed),
            reconnects = self.reconnects.load(Ordering::Relaxed),
            lost_events = self.lost_events.load(Ordering::Relaxed),
            "health"
        );
    }
}

/// Logs the health line every hour, from the first hour on: a slow leak shows as a rising rss_mib.
pub(crate) async fn health(stats: Arc<Stats>) {
    let mut every = tokio::time::interval(HEALTH_EVERY);
    every.tick().await;
    loop {
        every.tick().await;
        stats.log();
    }
}

// Resident memory from the kernel. Read in kB, not pages: the Pi 5's kernel uses 16 KiB pages.
fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// The short code a user is shown when something fails, and staff search the journal for: the
/// interaction id's last six digits.
pub(crate) fn reference_of(interaction_id: u64) -> String {
    const DIGITS: u64 = 1_000_000;
    format!("{:06}", interaction_id % DIGITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_the_last_six_digits_zero_padded() {
        assert_eq!(reference_of(1_311_000_000_000_123_456), "123456");
        assert_eq!(reference_of(1_311_000_000_000_000_042), "000042");
    }

    #[test]
    fn memory_is_read_from_the_kernel() {
        assert!(rss_kib().is_some_and(|kib| kib > 0));
    }

    #[test]
    fn counters_count() {
        let stats = Stats::new();
        stats.bump(Counter::Failed);
        stats.bump(Counter::Failed);
        assert_eq!(stats.failed.load(Ordering::Relaxed), 2);
    }
}
