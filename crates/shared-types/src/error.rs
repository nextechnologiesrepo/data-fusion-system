//! Shared error type for the fabric.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, FabricError>;

#[derive(Debug, Error)]
pub enum FabricError {
    #[error("schema validation failed: {0}")]
    Validation(String),

    #[error("observation rejected as stale: observed_at is {age_ms} ms old (limit {limit_ms} ms)")]
    StaleObservation { age_ms: i64, limit_ms: i64 },

    #[error("rate limit exceeded for source {source_id}")]
    RateLimited { source_id: String },

    #[error("not found: {0}")]
    NotFound(String),

    #[error("signature verification failed: {0}")]
    SignatureInvalid(String),

    #[error("provenance chain broken: {0}")]
    ProvenanceBroken(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}
