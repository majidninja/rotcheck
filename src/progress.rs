//! A one-line progress counter on stderr, for big trees.
//!
//! It is only drawn when stderr is a terminal, so logs, pipes and cron mail
//! never see it. The line is redrawn at most every [`INTERVAL`] and must be
//! [`clear`](Progress::clear)ed before anything else is printed, so it never
//! ends up in the middle of a report line.
//!
//! The methods take `&self` (the state lives in `Cell`s), so two closures
//! can share one counter: one that learns the total, one that ticks.

use std::cell::Cell;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

const INTERVAL: Duration = Duration::from_millis(100);

pub struct Progress {
    enabled: bool,
    total: Cell<Option<usize>>,
    done: Cell<usize>,
    last_draw: Cell<Option<Instant>>,
    shown: Cell<bool>,
}

impl Progress {
    /// A counter that draws only if `wanted` and stderr is a terminal.
    /// `total` is the number of files, if known up front.
    pub fn new(wanted: bool, total: Option<usize>) -> Self {
        Progress {
            enabled: wanted && io::stderr().is_terminal(),
            total: Cell::new(total),
            done: Cell::new(0),
            last_draw: Cell::new(None),
            shown: Cell::new(false),
        }
    }

    /// Sets the number of files, once it is known.
    pub fn set_total(&self, total: usize) {
        self.total.set(Some(total));
    }

    /// Counts one more file, redrawing the line if enough time has passed.
    pub fn tick(&self) {
        self.done.set(self.done.get() + 1);
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        if self.last_draw.get().is_some_and(|t| now - t < INTERVAL) {
            return;
        }
        self.last_draw.set(Some(now));
        self.shown.set(true);
        let mut err = io::stderr().lock();
        // `\r` back to the start, `ESC [K` erases the old text.
        let _ = write!(err, "\r\x1b[K{}", line(self.done.get(), self.total.get()));
        let _ = err.flush();
    }

    /// Removes the line, if one is on screen. Call before printing anything.
    pub fn clear(&self) {
        if self.shown.replace(false) {
            let mut err = io::stderr().lock();
            let _ = write!(err, "\r\x1b[K");
            let _ = err.flush();
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.clear();
    }
}

fn line(done: usize, total: Option<usize>) -> String {
    match total {
        Some(total) => format!("rotcheck: hashing [{done}/{total}]"),
        None => format!("rotcheck: checked {done} files"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_text() {
        assert_eq!(line(3, Some(10)), "rotcheck: hashing [3/10]");
        assert_eq!(line(7, None), "rotcheck: checked 7 files");
    }

    #[test]
    fn disabled_counts_but_never_shows() {
        let p = Progress::new(false, None);
        p.tick();
        p.tick();
        p.set_total(5);
        assert_eq!(p.done.get(), 2);
        assert_eq!(p.total.get(), Some(5));
        assert!(!p.shown.get());
    }
}
