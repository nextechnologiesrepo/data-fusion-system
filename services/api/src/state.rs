//! Shared application state and API error mapping.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use fusion_core::FusionEngine;
use ingestion::RateLimiter;
use provenance_store::InMemoryProvenanceStore;
use replay::ReplayReport;
use shared_types::FabricError;

/// Cloneable handle to the engine and its peripherals. The engine is single
/// threaded by contract, so it lives behind a `Mutex` and is only locked for the
/// duration of a synchronous operation (never across an `.await`).
#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Mutex<FusionEngine>>,
    pub provenance: Arc<InMemoryProvenanceStore>,
    pub rate_limiter: Arc<Mutex<RateLimiter>>,
    pub last_replay: Arc<Mutex<Option<ReplayReport>>>,
    pub started_at: Instant,
    pub scenarios_dir: PathBuf,
}

/// API-level error that renders as a JSON body with an HTTP status.
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        AppError {
            status,
            message: message.into(),
        }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        AppError::new(StatusCode::NOT_FOUND, message)
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        AppError::new(StatusCode::BAD_REQUEST, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        AppError::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl From<FabricError> for AppError {
    fn from(e: FabricError) -> Self {
        let status = match &e {
            FabricError::Validation(_) => StatusCode::BAD_REQUEST,
            FabricError::StaleObservation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            FabricError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            FabricError::NotFound(_) => StatusCode::NOT_FOUND,
            FabricError::SignatureInvalid(_) => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        AppError::new(status, e.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}
