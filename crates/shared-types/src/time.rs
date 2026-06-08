//! Logical time.
//!
//! The whole system runs on `i64` epoch-milliseconds rather than wall-clock
//! `DateTime` values. This keeps replay deterministic: a scenario file fixes
//! every timestamp, and the fusion path never calls "now" directly — it is
//! handed a [`Timestamp`] by an injected clock.

use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (UTC).
pub type Millis = i64;

/// A logical instant, milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub Millis);

impl Timestamp {
    pub const ZERO: Timestamp = Timestamp(0);

    #[inline]
    pub fn from_millis(ms: Millis) -> Self {
        Timestamp(ms)
    }

    #[inline]
    pub fn millis(self) -> Millis {
        self.0
    }

    /// Age in milliseconds relative to `now`. Negative if `self` is in the future.
    #[inline]
    pub fn age_ms(self, now: Timestamp) -> Millis {
        now.0 - self.0
    }

    #[inline]
    pub fn plus_ms(self, ms: Millis) -> Self {
        Timestamp(self.0 + ms)
    }
}

impl From<Millis> for Timestamp {
    fn from(ms: Millis) -> Self {
        Timestamp(ms)
    }
}
