//! Scenario format — a compact, deterministic synthetic event log.

use serde::{Deserialize, Serialize};

use shared_types::{FeedbackVerdict, HealthStatus, SourceKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub schema_version: u32,
    pub name: String,
    pub seed: u64,
    #[serde(default = "default_true")]
    pub deterministic: bool,
    /// Number of real targets, used by false/missed-track metrics.
    pub ground_truth_tracks: usize,
    #[serde(default)]
    pub config: ConfigSpec,
    pub timeline: Timeline,
    pub sources: Vec<SourceSpec>,
    #[serde(default)]
    pub health: Vec<HealthSpec>,
    pub emitters: Vec<EmitterSpec>,
    #[serde(default)]
    pub feedback: Vec<FeedbackSpec>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeline {
    pub start_ms: i64,
    pub end_ms: i64,
    pub step_ms: i64,
}

/// Optional engine-config overrides; omitted fields fall back to defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigSpec {
    pub staleness_limit_ms: Option<i64>,
    pub freshness_horizon_ms: Option<i64>,
    pub confirm_min_sources: Option<usize>,
    pub confirm_min_observations: Option<usize>,
    pub default_calibration_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSpec {
    pub source_id: String,
    pub kind: SourceKind,
    pub reliability: f64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSpec {
    pub source_id: String,
    pub status: HealthStatus,
    pub health_score: f64,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmitterKind {
    Radar,
    Eoir,
    EwSigint,
    Platform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TargetSpec {
    pub start: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitterSpec {
    pub source_id: String,
    pub kind: EmitterKind,
    pub seed: u64,
    pub target: TargetSpec,
    /// EO/IR classification label.
    #[serde(default)]
    pub classification: Option<String>,
    /// EW emitter centre frequency.
    #[serde(default)]
    pub frequency_mhz: Option<f64>,
    /// Backdate `observed_at` by this many ms to exercise stale rejection.
    #[serde(default)]
    pub observed_lag_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackSpec {
    pub at_ms: i64,
    pub track: String,
    pub operator: String,
    pub verdict: FeedbackVerdict,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub confidence_adjustment: Option<f64>,
}
