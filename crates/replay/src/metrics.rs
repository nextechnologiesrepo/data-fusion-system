//! The eight evaluation metrics.
//!
//! Pure functions of a [`MetricInputs`] snapshot so the scoring is itself
//! deterministic and unit-testable in isolation from the run.

use shared_types::{EvaluationMetric, MetricKind, SessionId, Timestamp, SCHEMA_VERSION};

/// Everything needed to score a completed replay run.
#[derive(Debug, Clone)]
pub struct MetricInputs {
    pub session_id: SessionId,
    pub computed_at: Timestamp,
    /// Number of real targets in the scenario.
    pub ground_truth_tracks: usize,
    pub confirmed_tracks: usize,
    /// Mean confidence across confirmed tracks.
    pub mean_confirmed_confidence: f64,
    pub confirmation_latencies_ms: Vec<i64>,
    pub stale_rejections: u64,
    pub total_conflicts: usize,
    pub accepted_observations: usize,
    pub operator_overrides: usize,
    /// Fraction of tracks with a complete provenance chain, in `[0, 1]`.
    pub provenance_completeness: f64,
}

fn metric(
    inp: &MetricInputs,
    kind: MetricKind,
    value: f64,
    unit: &str,
    detail: impl Into<String>,
) -> EvaluationMetric {
    EvaluationMetric {
        schema_version: SCHEMA_VERSION,
        session_id: inp.session_id.clone(),
        computed_at: inp.computed_at,
        metric: kind,
        value,
        unit: unit.to_string(),
        detail: detail.into(),
    }
}

pub fn compute(inp: &MetricInputs) -> Vec<EvaluationMetric> {
    let confirmed = inp.confirmed_tracks.max(0);
    let gt = inp.ground_truth_tracks;
    let accepted = inp.accepted_observations.max(1) as f64;

    // 1. mean track confirmation latency
    let latency = if inp.confirmation_latencies_ms.is_empty() {
        0.0
    } else {
        inp.confirmation_latencies_ms.iter().sum::<i64>() as f64
            / inp.confirmation_latencies_ms.len() as f64
    };

    // 2. false-track rate: confirmed beyond ground truth
    let false_tracks = confirmed.saturating_sub(gt);
    let false_rate = false_tracks as f64 / confirmed.max(1) as f64;

    // 3. missed-track rate: ground truth not confirmed
    let missed = gt.saturating_sub(confirmed);
    let missed_rate = missed as f64 / gt.max(1) as f64;

    // 4. confidence calibration error: |mean confidence - true-positive fraction|
    let tp_fraction = confirmed.min(gt) as f64 / confirmed.max(1) as f64;
    let calibration_error = (inp.mean_confirmed_confidence - tp_fraction).abs();

    // 6. conflict rate
    let conflict_rate = inp.total_conflicts as f64 / accepted;

    // 7. operator override rate
    let override_rate = inp.operator_overrides as f64 / accepted;

    vec![
        metric(
            inp,
            MetricKind::TrackConfirmationLatencyMs,
            latency,
            "ms",
            format!("mean over {} confirmed track(s)", inp.confirmation_latencies_ms.len()),
        ),
        metric(
            inp,
            MetricKind::FalseTrackRate,
            false_rate,
            "ratio",
            format!("{false_tracks} false / {confirmed} confirmed"),
        ),
        metric(
            inp,
            MetricKind::MissedTrackRate,
            missed_rate,
            "ratio",
            format!("{missed} missed / {gt} ground-truth"),
        ),
        metric(
            inp,
            MetricKind::ConfidenceCalibrationError,
            calibration_error,
            "abs_error",
            format!(
                "mean confidence {:.3} vs tp-fraction {:.3}",
                inp.mean_confirmed_confidence, tp_fraction
            ),
        ),
        metric(
            inp,
            MetricKind::StaleDataRejectionCount,
            inp.stale_rejections as f64,
            "count",
            "observations rejected as stale before fusion",
        ),
        metric(
            inp,
            MetricKind::ConflictRate,
            conflict_rate,
            "ratio",
            format!("{} conflicts / {} accepted obs", inp.total_conflicts, inp.accepted_observations),
        ),
        metric(
            inp,
            MetricKind::OperatorOverrideRate,
            override_rate,
            "ratio",
            format!("{} overrides / {} accepted obs", inp.operator_overrides, inp.accepted_observations),
        ),
        metric(
            inp,
            MetricKind::ProvenanceCompleteness,
            inp.provenance_completeness,
            "ratio",
            "fraction of tracks with a complete provenance chain",
        ),
    ]
}
