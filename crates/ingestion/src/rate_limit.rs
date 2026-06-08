//! Per-source token-bucket rate limiting.
//!
//! A v0 reliability guard: a misbehaving or spoofed source cannot flood the
//! fusion path. Tokens refill continuously based on logical time, so the limiter
//! is deterministic under replay just like everything else.

use std::collections::HashMap;

use shared_types::{FabricError, Result, SourceId, Timestamp};

struct Bucket {
    tokens: f64,
    last_ms: i64,
}

pub struct RateLimiter {
    capacity: f64,
    refill_per_ms: f64,
    buckets: HashMap<SourceId, Bucket>,
}

impl RateLimiter {
    /// `capacity` burst tokens, refilling at `rate_per_sec` tokens/second.
    pub fn new(capacity: f64, rate_per_sec: f64) -> Self {
        RateLimiter {
            capacity,
            refill_per_ms: rate_per_sec / 1000.0,
            buckets: HashMap::new(),
        }
    }

    /// Charge one token to `source`. Errors with [`FabricError::RateLimited`]
    /// when the bucket is empty.
    pub fn check(&mut self, source: &SourceId, now: Timestamp) -> Result<()> {
        let cap = self.capacity;
        let refill = self.refill_per_ms;
        let bucket = self.buckets.entry(source.clone()).or_insert(Bucket {
            tokens: cap,
            last_ms: now.millis(),
        });

        let elapsed = (now.millis() - bucket.last_ms).max(0) as f64;
        bucket.tokens = (bucket.tokens + elapsed * refill).min(cap);
        bucket.last_ms = now.millis();

        if bucket.tokens < 1.0 {
            return Err(FabricError::RateLimited {
                source_id: source.to_string(),
            });
        }
        bucket.tokens -= 1.0;
        Ok(())
    }
}
