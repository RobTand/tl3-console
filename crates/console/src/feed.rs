//! One LISTEN session per console. Every notification marks the board stale.
//! A lost session reconnects after a bounded backoff, then marks the board
//! stale, since changes may have committed while it was away. Sessions that
//! drop soon after opening keep backing off.

use crate::{
    describe,
    rebuild::{Change, Rebuilds},
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tl3_store::Listener;

/// Reconnect delays: doubling from the first, never above the cap.
pub(crate) struct Backoff {
    next: Duration,
}

impl Backoff {
    const FIRST: Duration = Duration::from_millis(100);
    const CAP: Duration = Duration::from_secs(5);
    /// A session open this long was healthy.
    pub(crate) const HEALTHY: Duration = Duration::from_secs(30);

    pub(crate) fn new() -> Self {
        Self { next: Self::FIRST }
    }

    pub(crate) fn delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = (delay * 2).min(Self::CAP);
        delay
    }

    /// A session that dropped soon after it opened keeps backing off; only a
    /// healthy one starts the delays over.
    pub(crate) fn after_session(&mut self, lasted: Duration) {
        if lasted >= Self::HEALTHY {
            self.reset();
        }
    }

    pub(crate) fn reset(&mut self) {
        self.next = Self::FIRST;
    }
}

/// Preserve postmortem invalidation even when its tables change with the world.
fn change(tables: &BTreeSet<String>) -> Change {
    let progress = |table: &str| matches!(table, "tl3_worker_progress" | "tl3_worker_summary");
    let postmortem = |table: &str| {
        matches!(
            table,
            "tl3_postmortems" | "tl3_postmortem_recommendations" | "tl3_code_grades"
        )
    };
    let postmortems = tables.iter().any(|table| postmortem(table));
    let world = tables
        .iter()
        .any(|table| !progress(table) && !postmortem(table) && table != "tl3_console_commands");
    match (world, postmortems) {
        (true, true) => Change::WorldPostmortems,
        (true, false) => Change::World,
        (false, true) => Change::Postmortems,
        (false, false) if tables.contains("tl3_console_commands") => Change::Commands,
        (false, false) => Change::Progress,
    }
}

pub(crate) fn run(
    settings: tl3_config::Store,
    rebuilds: Arc<Rebuilds>,
    connected: Arc<AtomicBool>,
) {
    let mut backoff = Backoff::new();
    let mut reported = false;
    loop {
        let opened = Instant::now();
        match Listener::open(&settings) {
            Ok(mut listener) => {
                reported = false;
                connected.store(true, Ordering::SeqCst);
                rebuilds.request(Change::WorldPostmortems);
                let error = loop {
                    match listener.wait() {
                        Ok(tables) => rebuilds.request(change(&tables)),
                        Err(error) => break error,
                    }
                };
                connected.store(false, Ordering::SeqCst);
                eprintln!(
                    "console change feed lost: {}; reconnecting",
                    describe(&error)
                );
                rebuilds.request(Change::World);
                backoff.after_session(opened.elapsed());
            }
            Err(error) if !reported => {
                reported = true;
                eprintln!(
                    "console change feed unavailable: {}; retrying",
                    describe(&error)
                );
            }
            Err(_) => {}
        }
        thread::sleep(backoff.delay());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filed_commands_alone_reread_commands_not_the_world() {
        let tables = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        assert_eq!(change(&tables(&["tl3_worker_progress"])), Change::Progress);
        assert_eq!(change(&tables(&["tl3_console_commands"])), Change::Commands);
        assert_eq!(
            change(&tables(&["tl3_console_commands", "tl3_worker_summary"])),
            Change::Commands
        );
        // tl3d settles commands in its own commits, so a commit rereads them.
        assert_eq!(
            change(&tables(&["tl3_meta", "tl3_console_commands"])),
            Change::World
        );
    }

    #[test]
    fn backoff_doubles_to_its_cap_and_resets() {
        let mut backoff = Backoff::new();
        let delays: Vec<_> = (0..9).map(|_| backoff.delay().as_millis()).collect();
        assert_eq!(delays, [100, 200, 400, 800, 1600, 3200, 5000, 5000, 5000]);
        backoff.reset();
        assert_eq!(backoff.delay(), Duration::from_millis(100));
    }

    #[test]
    fn sessions_that_drop_at_once_keep_backing_off() {
        let mut backoff = Backoff::new();
        let mut delays = vec![];
        for _ in 0..4 {
            backoff.after_session(Duration::from_millis(20));
            delays.push(backoff.delay().as_millis());
        }
        assert_eq!(delays, [100, 200, 400, 800]);
        backoff.after_session(Backoff::HEALTHY);
        assert_eq!(backoff.delay(), Duration::from_millis(100));
    }
}
