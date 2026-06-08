//! The canonical objects of the fabric.
//!
//! These are the only types that cross crate boundaries on the data path.
//! They are intentionally plain: serde-serializable, no behaviour beyond
//! constructors and small helpers. Engines operate *on* them; they do not
//! live *inside* them.

use serde::{Deserialize, Serialize};

use crate::ids::*;
use crate::time::Timestamp;
use crate::SCHEMA_VERSION;

// ---------------------------------------------------------------------------
// Common building blocks
// ---------------------------------------------------------------------------

/// Placeholder for a signed-event envelope (v0 — see threat model).
///
/// In v0 every event is "signed" with the `none-v0` algorithm and verification
/// is a no-op. The shape is here so the wire format and storage do not change
/// when real signing (e.g. Ed25519 over the canonical JSON) is wired in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub algorithm: String,
    pub key_id: String,
    /// Hex-encoded signature bytes. Empty for `none-v0`.
    pub value: String,
}

impl Signature {
    pub fn unsigned() -> Self {
        Signature {
            algorithm: "none-v0".to_string(),
            key_id: "unsigned".to_string(),
            value: String::new(),
        }
    }

    /// v0 verification: the placeholder algorithm always verifies.
    /// Real algorithms return `false` until implemented, so they fail closed.
    pub fn verify(&self, _payload: &[u8]) -> bool {
        self.algorithm == "none-v0"
    }
}

impl Default for Signature {
    fn default() -> Self {
        Signature::unsigned()
    }
}

/// A normalized kinematic state in a local ENU frame (metres, metres/second).
///
/// Adapters translate their native payloads into this so the fusion core can
/// gate and associate generically without knowing about radar vs EO/IR vs EW.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StateEstimate {
    /// East, North, Up position in metres.
    pub position: [f64; 3],
    /// Velocity in metres/second.
    pub velocity: [f64; 3],
    /// 1-sigma positional uncertainty in metres.
    pub position_sigma_m: f64,
}

impl StateEstimate {
    pub fn at(position: [f64; 3], position_sigma_m: f64) -> Self {
        StateEstimate {
            position,
            velocity: [0.0, 0.0, 0.0],
            position_sigma_m,
        }
    }

    /// Euclidean distance between two position estimates, in metres.
    pub fn distance_to(&self, other: &StateEstimate) -> f64 {
        let dx = self.position[0] - other.position[0];
        let dy = self.position[1] - other.position[1];
        let dz = self.position[2] - other.position[2];
        (dx * dx + dy * dy + dz * dz).sqrt()
    }
}

// ---------------------------------------------------------------------------
// 1. Source
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// Radar-like track observations.
    Radar,
    /// EO/IR-like detections.
    EoIr,
    /// EW/SIGINT-like emitter observations.
    EwSigint,
    /// Platform state / PNT-like telemetry.
    Platform,
    /// Human feedback / operator console.
    Operator,
}

/// A registered data source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub schema_version: u32,
    pub source_id: SourceId,
    pub kind: SourceKind,
    pub name: String,
    /// Prior reliability in `[0, 1]`, used by the confidence engine.
    pub reliability: f64,
    pub registered_at: Timestamp,
}

impl Source {
    pub fn new(
        source_id: SourceId,
        kind: SourceKind,
        name: impl Into<String>,
        reliability: f64,
        registered_at: Timestamp,
    ) -> Self {
        Source {
            schema_version: SCHEMA_VERSION,
            source_id,
            kind,
            name: name.into(),
            reliability: reliability.clamp(0.0, 1.0),
            registered_at,
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Observation
// ---------------------------------------------------------------------------

/// Native, source-specific payload. Kept alongside the normalized
/// [`StateEstimate`] so nothing is lost in translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationPayload {
    RadarTrack {
        range_m: f64,
        bearing_deg: f64,
        elevation_deg: f64,
        radial_velocity_mps: f64,
    },
    EoIrDetection {
        bearing_deg: f64,
        elevation_deg: f64,
        classification: String,
        pixel_intensity: f64,
    },
    EwEmitter {
        bearing_deg: f64,
        frequency_mhz: f64,
        modulation: String,
        signal_strength_dbm: f64,
    },
    PlatformState {
        lat_deg: f64,
        lon_deg: f64,
        alt_m: f64,
        heading_deg: f64,
        pnt_quality: f64,
    },
    /// Operator override delivered through the ingestion path.
    OperatorOverride {
        target_track: Option<TrackId>,
        directive: String,
    },
}

/// A single observation emitted by a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub schema_version: u32,
    pub observation_id: ObservationId,
    pub source_id: SourceId,
    pub source_kind: SourceKind,
    /// When the phenomenon was observed (drives staleness + freshness).
    pub observed_at: Timestamp,
    /// When the fabric received the observation.
    pub received_at: Timestamp,
    pub payload: ObservationPayload,
    /// Normalized kinematics for fusion. `None` for non-kinematic payloads.
    pub state: Option<StateEstimate>,
    /// Source-reported measurement confidence in `[0, 1]`.
    pub measurement_confidence: f64,
    pub provenance_ref: Option<ProvenanceId>,
    pub signature: Signature,
}

// ---------------------------------------------------------------------------
// 3. SensorHealth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Nominal,
    Degraded,
    Faulted,
    Offline,
}

/// A health report for a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensorHealth {
    pub schema_version: u32,
    pub source_id: SourceId,
    pub reported_at: Timestamp,
    pub status: HealthStatus,
    /// Health score in `[0, 1]`, fed into the confidence engine.
    pub health_score: f64,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// 4. ProvenanceRecord (append-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceOp {
    /// A new track hypothesis was created.
    Created,
    /// An observation was merged into an existing track.
    Merged,
    /// The track was re-scored / re-stated without new observations.
    Updated,
    /// A conflicting observation was preserved rather than merged.
    ConflictPreserved,
    /// Operator feedback altered the track.
    OperatorOverride,
}

/// An append-only record describing one change to a fused track.
///
/// Records form a hash-free chain via `prev_provenance_id`; reading the chain
/// backwards from a track's latest record answers "what changed since the
/// previous fused version?".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub schema_version: u32,
    pub provenance_id: ProvenanceId,
    pub track_id: TrackId,
    /// The fused-track version this record produced.
    pub fused_version: u64,
    pub created_at: Timestamp,
    pub operation: ProvenanceOp,
    pub contributing_observations: Vec<ObservationId>,
    pub contributing_sources: Vec<SourceId>,
    pub confidence_before: Option<f64>,
    pub confidence_after: f64,
    pub notes: String,
    /// Link to the previous record for this track, forming the audit chain.
    pub prev_provenance_id: Option<ProvenanceId>,
}

// ---------------------------------------------------------------------------
// 5. ConfidenceVector
// ---------------------------------------------------------------------------

/// Machine-readable reason codes. Stable identifiers, safe to switch on
/// downstream. The human-readable detail lives in `degradation_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    SingleSourceOnly,
    MultiSourceCorroboration,
    StaleContribution,
    SensorDegraded,
    ConflictDetected,
    OperatorConfirmed,
    OperatorRejected,
    LowSourceReliability,
    WellCalibrated,
    CalibrationDrift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStatus {
    Calibrated,
    Drifting,
    Uncalibrated,
}

/// The seven normalized inputs the confidence score is computed from, retained
/// so an operator can see exactly which lever moved the score.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceComponents {
    pub source_reliability: f64,
    pub sensor_health: f64,
    pub freshness: f64,
    pub corroboration: f64,
    /// `1.0` = no conflict, `0.0` = maximum conflict.
    pub conflict: f64,
    pub calibration_score: f64,
    /// `0.5` = neutral, `>0.5` operator-boosted, `<0.5` operator-suppressed.
    pub operator_feedback: f64,
}

/// The confidence engine's output for a fused track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceVector {
    pub schema_version: u32,
    pub computed_at: Timestamp,
    /// Aggregate confidence in `[0, 1]`.
    pub score: f64,
    /// Inclusive `[low, high]` band around `score`.
    pub uncertainty_band: [f64; 2],
    pub reason_codes: Vec<ReasonCode>,
    /// Human-readable explanation when the score is degraded.
    pub degradation_reason: Option<String>,
    pub calibration_status: CalibrationStatus,
    pub components: ConfidenceComponents,
}

// ---------------------------------------------------------------------------
// 6. TrackHypothesis
// ---------------------------------------------------------------------------

/// A candidate track under consideration by the fusion core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackHypothesis {
    pub schema_version: u32,
    pub hypothesis_id: HypothesisId,
    pub track_id: TrackId,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub state: StateEstimate,
    pub supporting_observations: Vec<ObservationId>,
    pub conflicting_observations: Vec<ObservationId>,
    pub source_ids: Vec<SourceId>,
}

// ---------------------------------------------------------------------------
// 7. FusedTrack
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackStatus {
    Tentative,
    Confirmed,
    Coasting,
    Dropped,
}

/// A retained conflicting observation. Conflicts are preserved, never hidden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub observation_id: ObservationId,
    pub source_id: SourceId,
    pub reason: String,
    /// How far the conflicting observation sat from the track, in metres.
    pub divergence_m: f64,
}

/// The published, machine-readable fused track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FusedTrack {
    pub schema_version: u32,
    pub track_id: TrackId,
    pub version: u64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub state: StateEstimate,
    pub confidence: ConfidenceVector,
    pub provenance_ref: ProvenanceId,
    pub contributing_observations: Vec<ObservationId>,
    pub contributing_sources: Vec<SourceId>,
    pub conflicts: Vec<ConflictRecord>,
    pub status: TrackStatus,
}

// ---------------------------------------------------------------------------
// 8. RecommendationCue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueSeverity {
    Info,
    Caution,
    Warning,
}

/// A decision-support cue surfaced to a human operator. Advisory only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecommendationCue {
    pub schema_version: u32,
    pub cue_id: CueId,
    pub track_id: TrackId,
    pub created_at: Timestamp,
    pub severity: CueSeverity,
    pub message: String,
    /// Snapshot of the track confidence at cue time.
    pub confidence_ref: f64,
    pub recommended_action: String,
}

// ---------------------------------------------------------------------------
// 9. OperatorFeedback
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackVerdict {
    ConfirmTrack,
    RejectTrack,
    Reclassify,
    AdjustConfidence,
}

/// Operator feedback / override on a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperatorFeedback {
    pub schema_version: u32,
    pub feedback_id: FeedbackId,
    pub track_id: TrackId,
    pub operator_id: String,
    pub submitted_at: Timestamp,
    pub verdict: FeedbackVerdict,
    pub note: String,
    /// Optional explicit adjustment in `[-1, 1]` for `AdjustConfidence`.
    pub confidence_adjustment: Option<f64>,
}

// ---------------------------------------------------------------------------
// 10. ReplaySession
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayStatus {
    Pending,
    Running,
    Completed,
    Aborted,
}

/// A single run of the replay harness over a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaySession {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub scenario_name: String,
    pub started_at: Timestamp,
    pub finished_at: Option<Timestamp>,
    pub deterministic: bool,
    pub seed: u64,
    pub status: ReplayStatus,
    pub event_count: u64,
}

// ---------------------------------------------------------------------------
// 11. EvaluationMetric
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    TrackConfirmationLatencyMs,
    FalseTrackRate,
    MissedTrackRate,
    ConfidenceCalibrationError,
    StaleDataRejectionCount,
    ConflictRate,
    OperatorOverrideRate,
    ProvenanceCompleteness,
}

/// One computed evaluation metric tied to a replay session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationMetric {
    pub schema_version: u32,
    pub session_id: SessionId,
    pub computed_at: Timestamp,
    pub metric: MetricKind,
    pub value: f64,
    pub unit: String,
    pub detail: String,
}
