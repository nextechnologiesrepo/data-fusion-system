//! Request and response shapes for the API.
//!
//! Wherever possible the canonical [`shared_types`] objects are returned
//! directly; these DTOs only exist where the wire shape differs (e.g. clients
//! submit feedback without server-assigned IDs) or where a query result type is
//! not itself serializable.

use serde::{Deserialize, Serialize};

use fusion_core::FusionOutcome;
use provenance_store::{ConfidenceImpact, ProvenanceDiff};
use shared_types::{
    ConfidenceVector, EvaluationMetric, FeedbackVerdict, ProvenanceRecord, RecommendationCue,
    ReplaySession,
};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
    pub uptime_ms: u128,
}

#[derive(Debug, Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub tracks: usize,
}

/// Result of submitting an observation or feedback.
#[derive(Debug, Serialize)]
pub struct IngestResponse {
    pub accepted: bool,
    pub rejection: Option<String>,
    pub operation: Option<String>,
    pub track_id: Option<String>,
    pub provenance_id: Option<String>,
    pub cue: Option<RecommendationCue>,
    pub note: Option<String>,
}

impl From<FusionOutcome> for IngestResponse {
    fn from(o: FusionOutcome) -> Self {
        IngestResponse {
            accepted: o.accepted,
            rejection: o.rejection.map(|r| format!("{r:?}")),
            operation: o.operation.map(|op| format!("{op:?}")),
            track_id: o.track.as_ref().map(|t| t.track_id.to_string()),
            provenance_id: o.provenance.as_ref().map(|p| p.provenance_id.to_string()),
            cue: o.cue,
            note: o.note,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub track_id: String,
    pub operator_id: String,
    pub verdict: FeedbackVerdict,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub confidence_adjustment: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ConfidenceImpactDto {
    pub provenance_id: String,
    pub operation: String,
    pub sources: Vec<String>,
    pub confidence_delta: f64,
}

impl From<ConfidenceImpact> for ConfidenceImpactDto {
    fn from(i: ConfidenceImpact) -> Self {
        ConfidenceImpactDto {
            provenance_id: i.provenance_id.to_string(),
            operation: format!("{:?}", i.operation),
            sources: i.sources.iter().map(|s| s.to_string()).collect(),
            confidence_delta: i.confidence_delta,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProvenanceDiffDto {
    pub from_version: u64,
    pub to_version: u64,
    pub operation: String,
    pub added_observations: Vec<String>,
    pub added_sources: Vec<String>,
    pub confidence_delta: f64,
    pub notes: String,
}

impl From<ProvenanceDiff> for ProvenanceDiffDto {
    fn from(d: ProvenanceDiff) -> Self {
        ProvenanceDiffDto {
            from_version: d.from_version,
            to_version: d.to_version,
            operation: format!("{:?}", d.operation),
            added_observations: d.added_observations.iter().map(|o| o.to_string()).collect(),
            added_sources: d.added_sources.iter().map(|s| s.to_string()).collect(),
            confidence_delta: d.confidence_delta,
            notes: d.notes,
        }
    }
}

/// Answers the four provenance questions for one track.
#[derive(Debug, Serialize)]
pub struct ProvenanceResponse {
    pub track_id: String,
    pub why_exists: ProvenanceRecord,
    pub contributing_observations: Vec<String>,
    pub changed_since_previous: Option<ProvenanceDiffDto>,
    pub sources_that_lowered_confidence: Vec<ConfidenceImpactDto>,
    pub chain: Vec<ProvenanceRecord>,
}

#[derive(Debug, Serialize)]
pub struct ConfidenceExplanationResponse {
    pub track_id: String,
    pub confidence: ConfidenceVector,
    pub sources_that_lowered_confidence: Vec<ConfidenceImpactDto>,
}

#[derive(Debug, Deserialize)]
pub struct ReplayStartRequest {
    pub scenario: String,
}

#[derive(Debug, Serialize)]
pub struct ReplayStartResponse {
    pub session: ReplaySession,
    pub track_count: usize,
    pub metrics: Vec<EvaluationMetric>,
}

#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub session: Option<ReplaySession>,
    pub metrics: Vec<EvaluationMetric>,
}
