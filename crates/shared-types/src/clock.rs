//! Time source abstraction.
//!
//! Nothing on the fusion path calls the OS clock directly. A [`Clock`] is
//! injected, so live operation uses [`SystemClock`] while replay uses
//! [`ManualClock`] driven by the scenario's timestamps — making replay bit-for-bit
//! reproducible.

use std::sync::atomic::{AtomicI64, Ordering};

use crate::time::{Millis, Timestamp};

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Wall-clock time. Used in live operation only.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as Millis)
            .unwrap_or(0);
        Timestamp(ms)
    }
}

/// A clock whose value is set explicitly. Used by replay so that "now" advances
/// in lockstep with scenario events rather than wall time.
#[derive(Debug)]
pub struct ManualClock {
    now_ms: AtomicI64,
}

impl ManualClock {
    pub fn new(start: Timestamp) -> Self {
        ManualClock {
            now_ms: AtomicI64::new(start.0),
        }
    }

    /// Advance (or rewind) to an explicit instant.
    pub fn set(&self, t: Timestamp) {
        self.now_ms.store(t.0, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        Timestamp(self.now_ms.load(Ordering::SeqCst))
    }
}
