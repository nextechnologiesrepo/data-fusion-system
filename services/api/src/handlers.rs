//! HTTP handlers. Thin glue over the engine, store, and replay harness.

use axum::extract::{Path, State};
use axum::Json;

use ingestion::validate;
use provenance_store::{ProvenanceQuery, ProvenanceStore};
use shared_types::{
    Clock, FusedTrack, Observation, OperatorFeedback, Source, SystemClock, Timestamp, TrackId,
    SCHEMA_VERSION,
};

use crate::dto::*;
use crate::state::{AppError, AppState};

type ApiResult<T> = Result<Json<T>, AppError>;

fn lock_engine(
    state: &AppState,
) -> Result<std::sync::MutexGuard<'_, fusion_core::FusionEngine>, AppError> {
    state
        .engine
        .lock()
        .map_err(|_| AppError::internal("engine mutex poisoned"))
}

// --- health -------------------------------------------------------------

pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: SCHEMA_VERSION,
        uptime_ms: state.started_at.elapsed().as_millis(),
    })
}

pub async fn ready(State(state): State<AppState>) -> ApiResult<ReadyResponse> {
    let engine = lock_engine(&state)?;
    Ok(Json(ReadyResponse {
        ready: true,
        tracks: engine.all_tracks().len(),
    }))
}

// --- sources ------------------------------------------------------------

pub async fn list_sources(State(state): State<AppState>) -> ApiResult<Vec<Source>> {
    let engine = lock_engine(&state)?;
    Ok(Json(engine.list_sources()))
}

// --- observations -------------------------------------------------------

pub async fn submit_observation(
    State(state): State<AppState>,
    Json(mut obs): Json<Observation>,
) -> ApiResult<IngestResponse> {
    // Fill receive time if the client did not stamp it.
    if obs.received_at == Timestamp::ZERO {
        obs.received_at = SystemClock.now();
    }

    // Rate limit per source, then validate, then fuse.
    {
        let mut rl = state
            .rate_limiter
            .lock()
            .map_err(|_| AppError::internal("rate limiter poisoned"))?;
        rl.check(&obs.source_id, obs.received_at)?;
    }
    validate(&obs)?;

    let outcome = {
        let mut engine = lock_engine(&state)?;
        engine.process_observation(obs)?
    };
    Ok(Json(IngestResponse::from(outcome)))
}

// --- tracks -------------------------------------------------------------

pub async fn list_tracks(State(state): State<AppState>) -> ApiResult<Vec<FusedTrack>> {
    let engine = lock_engine(&state)?;
    Ok(Json(engine.all_tracks()))
}

pub async fn get_track(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<FusedTrack> {
    let engine = lock_engine(&state)?;
    engine
        .get_track(&TrackId::new(id.clone()))
        .map(Json)
        .ok_or_else(|| AppError::not_found(format!("track {id}")))
}

pub async fn get_provenance(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ProvenanceResponse> {
    let track = TrackId::new(id.clone());
    let store = &state.provenance;

    let chain = store.chain_for_track(&track)?;
    if chain.is_empty() {
        return Err(AppError::not_found(format!("no provenance for track {id}")));
    }
    let why_exists = store.why_does_track_exist(&track)?;
    let contributing_observations = store
        .contributing_observations(&track)?
        .iter()
        .map(|o| o.to_string())
        .collect();
    let changed_since_previous = store
        .changed_since_previous(&track)?
        .map(ProvenanceDiffDto::from);
    let sources_that_lowered_confidence = store
        .sources_that_lowered_confidence(&track)?
        .into_iter()
        .map(ConfidenceImpactDto::from)
        .collect();

    Ok(Json(ProvenanceResponse {
        track_id: id,
        why_exists,
        contributing_observations,
        changed_since_previous,
        sources_that_lowered_confidence,
        chain,
    }))
}

pub async fn get_confidence(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ConfidenceExplanationResponse> {
    let track_id = TrackId::new(id.clone());
    let confidence = {
        let engine = lock_engine(&state)?;
        engine
            .get_track(&track_id)
            .map(|t| t.confidence)
            .ok_or_else(|| AppError::not_found(format!("track {id}")))?
    };
    let sources_that_lowered_confidence = state
        .provenance
        .sources_that_lowered_confidence(&track_id)?
        .into_iter()
        .map(ConfidenceImpactDto::from)
        .collect();

    Ok(Json(ConfidenceExplanationResponse {
        track_id: id,
        confidence,
        sources_that_lowered_confidence,
    }))
}

// --- operator feedback --------------------------------------------------

pub async fn submit_feedback(
    State(state): State<AppState>,
    Json(req): Json<FeedbackRequest>,
) -> ApiResult<IngestResponse> {
    let now = SystemClock.now();
    let fb = OperatorFeedback {
        schema_version: SCHEMA_VERSION,
        feedback_id: shared_types::FeedbackId::new(format!("fbk-{}", now.millis())),
        track_id: TrackId::new(req.track_id),
        operator_id: req.operator_id,
        submitted_at: now,
        verdict: req.verdict,
        note: req.note,
        confidence_adjustment: req.confidence_adjustment,
    };
    let outcome = {
        let mut engine = lock_engine(&state)?;
        engine.apply_operator_feedback(fb)?
    };
    Ok(Json(IngestResponse::from(outcome)))
}

// --- replay + metrics ---------------------------------------------------

pub async fn replay_start(
    State(state): State<AppState>,
    Json(req): Json<ReplayStartRequest>,
) -> ApiResult<ReplayStartResponse> {
    let path = state.scenarios_dir.join(format!("{}.json", req.scenario));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| AppError::not_found(format!("scenario '{}': {e}", req.scenario)))?;
    let scenario: replay::Scenario =
        serde_json::from_str(&text).map_err(|e| AppError::bad_request(e.to_string()))?;

    let report = replay::ReplayHarness::new().run(&scenario)?;
    let response = ReplayStartResponse {
        session: report.session.clone(),
        track_count: report.tracks.len(),
        metrics: report.metrics.clone(),
    };
    *state
        .last_replay
        .lock()
        .map_err(|_| AppError::internal("replay state poisoned"))? = Some(report);
    Ok(Json(response))
}

pub async fn replay_stop(State(state): State<AppState>) -> ApiResult<MetricsResponse> {
    // v0 replay runs synchronously, so "stop" simply reports the last result.
    let last = state
        .last_replay
        .lock()
        .map_err(|_| AppError::internal("replay state poisoned"))?;
    Ok(Json(MetricsResponse {
        session: last.as_ref().map(|r| r.session.clone()),
        metrics: last.as_ref().map(|r| r.metrics.clone()).unwrap_or_default(),
    }))
}

pub async fn get_metrics(State(state): State<AppState>) -> ApiResult<MetricsResponse> {
    let last = state
        .last_replay
        .lock()
        .map_err(|_| AppError::internal("replay state poisoned"))?;
    Ok(Json(MetricsResponse {
        session: last.as_ref().map(|r| r.session.clone()),
        metrics: last.as_ref().map(|r| r.metrics.clone()).unwrap_or_default(),
    }))
}
