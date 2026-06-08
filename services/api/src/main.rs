//! `fusion-api` — the Axum service that exposes the fabric.
//!
//! Security baseline: binds to `127.0.0.1` by default (local-only), no secrets
//! are read from the binary, every inbound observation is validated and
//! rate-limited, and all access goes through the structured-logged handlers.
//! Override the bind address with `FUSION_BIND` only when you mean to.

mod dto;
mod handlers;
mod state;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeFile;
use tower_http::trace::TraceLayer;

use fusion_core::{
    BaselineConfidenceScorer, FusionConfig, FusionEngine, InMemoryProvenanceStore,
    NearestNeighborPolicy,
};
use ingestion::RateLimiter;

use crate::state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    // Assemble the proprietary core: nearest-neighbour policy + baseline scorer
    // + append-only in-memory provenance. All replaceable behind their traits.
    let store = Arc::new(InMemoryProvenanceStore::new());
    let engine = FusionEngine::new(
        Box::new(NearestNeighborPolicy::default()),
        Arc::new(BaselineConfidenceScorer::new()),
        store.clone(),
        FusionConfig::default(),
    );

    let scenarios_dir = PathBuf::from(
        std::env::var("FUSION_SCENARIOS_DIR").unwrap_or_else(|_| "sim/scenarios".to_string()),
    );
    let openapi_path =
        std::env::var("FUSION_OPENAPI").unwrap_or_else(|_| "docs/openapi.yaml".to_string());

    let state = AppState {
        engine: Arc::new(Mutex::new(engine)),
        provenance: store,
        // 200-token burst, refilling 100/s per source.
        rate_limiter: Arc::new(Mutex::new(RateLimiter::new(200.0, 100.0))),
        last_replay: Arc::new(Mutex::new(None)),
        started_at: Instant::now(),
        scenarios_dir,
    };

    let app = Router::new()
        .route("/health", get(handlers::health))
        .route("/healthz", get(handlers::health))
        .route("/readyz", get(handlers::ready))
        .route("/api/v1/sources", get(handlers::list_sources))
        .route("/api/v1/observations", post(handlers::submit_observation))
        .route("/api/v1/tracks", get(handlers::list_tracks))
        .route("/api/v1/tracks/:id", get(handlers::get_track))
        .route("/api/v1/tracks/:id/provenance", get(handlers::get_provenance))
        .route("/api/v1/tracks/:id/confidence", get(handlers::get_confidence))
        .route("/api/v1/feedback", post(handlers::submit_feedback))
        .route("/api/v1/replay/start", post(handlers::replay_start))
        .route("/api/v1/replay/stop", post(handlers::replay_stop))
        .route("/api/v1/metrics", get(handlers::get_metrics))
        .route_service("/openapi.yaml", ServeFile::new(openapi_path))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = std::env::var("FUSION_BIND").unwrap_or_else(|_| "127.0.0.1:8088".to_string());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind FUSION_BIND address");
    tracing::info!(address = %addr, "fusion-api listening (local-only default)");
    axum::serve(listener, app)
        .await
        .expect("server error");
}
