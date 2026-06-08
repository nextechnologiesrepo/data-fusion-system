//! Ingestion: the replaceable edge of the system.
//!
//! Adapters, validation, and rate limiting live here precisely because they are
//! *not* the proprietary core — they are expected to be swapped for real sensor
//! protocols later. v0 ships synthetic, deterministic mock generators for all
//! five source families plus a strict validator and a per-source token-bucket
//! rate limiter.

pub mod adapter;
pub mod generators;
pub mod rate_limit;
pub mod rng;
pub mod validate;

pub use adapter::SourceAdapter;
pub use generators::{
    EoIrGenerator, EwEmitterGenerator, OperatorOverrideGenerator, PlatformStateGenerator,
    RadarGenerator, SyntheticTarget,
};
pub use rate_limit::RateLimiter;
pub use rng::DeterministicRng;
pub use validate::validate;
