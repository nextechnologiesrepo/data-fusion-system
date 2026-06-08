//! Confidence and uncertainty layer.
//!
//! Deliberately a **separate engine** from fusion. Fusion decides *what* a track
//! is; this decides *how much to trust it* and *why*. It takes a flat
//! [`ConfidenceInputs`] (so it has no dependency on fusion internals) and emits a
//! [`ConfidenceVector`] carrying a score, an uncertainty band, machine-readable
//! reason codes, a degradation explanation, and a calibration status.
//!
//! The v0 [`BaselineConfidenceScorer`] is intentionally simple and fully
//! deterministic: same inputs → same output, every time. It is meant to be
//! explainable and easy to replace with a learned model later behind the same
//! [`ConfidenceScorer`] trait.

use shared_types::{
    CalibrationStatus, ConfidenceComponents, ConfidenceVector, ReasonCode, Timestamp,
    SCHEMA_VERSION,
};

/// How an operator's feedback influences confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OperatorInfluence {
    /// No operator input.
    Neutral,
    /// Operator confirmed the track.
    Confirmed,
    /// Operator rejected the track.
    Rejected,
    /// Operator nudged confidence by an explicit amount in `[-1, 1]`.
    Adjusted(f64),
}

impl OperatorInfluence {
    /// Normalized component value in `[0, 1]` (0.5 = neutral).
    fn component(self) -> f64 {
        match self {
            OperatorInfluence::Neutral => 0.5,
            OperatorInfluence::Confirmed => 0.9,
            OperatorInfluence::Rejected => 0.1,
            OperatorInfluence::Adjusted(x) => (0.5 + x / 2.0).clamp(0.0, 1.0),
        }
    }
}

/// Flat set of signals the scorer needs. The fusion core assembles this from a
/// track's contributing observations, source registry, and health reports.
#[derive(Debug, Clone)]
pub struct ConfidenceInputs {
    pub now: Timestamp,
    /// Reliability priors of each distinct contributing source, in `[0, 1]`.
    pub source_reliabilities: Vec<f64>,
    /// Health scores of each contributing source, in `[0, 1]`.
    pub source_health: Vec<f64>,
    /// Age (ms) of the freshest contributing observation.
    pub freshest_age_ms: i64,
    /// Number of distinct corroborating sources.
    pub distinct_sources: usize,
    pub support_count: usize,
    pub conflict_count: usize,
    /// Historical calibration score in `[0, 1]`; `0.0` means no history yet.
    pub calibration_score: f64,
    pub operator: OperatorInfluence,
    /// Observations older than this contribute ~0 freshness.
    pub freshness_horizon_ms: i64,
}

impl ConfidenceInputs {
    /// A minimal single-source input, handy for tests and stubs.
    pub fn single_source(now: Timestamp, reliability: f64, health: f64) -> Self {
        ConfidenceInputs {
            now,
            source_reliabilities: vec![reliability],
            source_health: vec![health],
            freshest_age_ms: 0,
            distinct_sources: 1,
            support_count: 1,
            conflict_count: 0,
            calibration_score: 0.0,
            operator: OperatorInfluence::Neutral,
            freshness_horizon_ms: 10_000,
        }
    }
}

/// Pluggable confidence scorer.
pub trait ConfidenceScorer: Send + Sync {
    fn score(&self, inputs: &ConfidenceInputs) -> ConfidenceVector;
    fn name(&self) -> &'static str;
}

/// Component weights for the baseline scorer (sum to 1.0, operator applied separately).
struct Weights;
impl Weights {
    const RELIABILITY: f64 = 0.20;
    const HEALTH: f64 = 0.15;
    const FRESHNESS: f64 = 0.15;
    const CORROBORATION: f64 = 0.20;
    const CONFLICT: f64 = 0.15;
    const CALIBRATION: f64 = 0.15;
}

/// The v0 deterministic scorer.
#[derive(Debug, Default, Clone, Copy)]
pub struct BaselineConfidenceScorer;

impl BaselineConfidenceScorer {
    pub fn new() -> Self {
        BaselineConfidenceScorer
    }
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

fn worst(xs: &[f64]) -> f64 {
    xs.iter().copied().fold(f64::INFINITY, f64::min).clamp(0.0, 1.0)
}

impl ConfidenceScorer for BaselineConfidenceScorer {
    fn score(&self, inputs: &ConfidenceInputs) -> ConfidenceVector {
        // --- normalize the seven inputs into components in [0, 1] -----------
        let reliability = mean(&inputs.source_reliabilities);
        let health = if inputs.source_health.is_empty() {
            0.0
        } else {
            worst(&inputs.source_health)
        };
        let horizon = inputs.freshness_horizon_ms.max(1) as f64;
        let freshness = (1.0 - inputs.freshest_age_ms as f64 / horizon).clamp(0.0, 1.0);
        // 1 source -> 0.40, 2 -> 0.70, 3 -> 0.85, 4 -> 0.925 ...
        let corroboration = if inputs.distinct_sources == 0 {
            0.0
        } else {
            1.0 - 0.6 * 0.5_f64.powi(inputs.distinct_sources as i32 - 1)
        };
        let total_obs = inputs.support_count + inputs.conflict_count;
        let conflict = if total_obs == 0 {
            1.0
        } else {
            inputs.support_count as f64 / total_obs as f64
        };
        let calibration = inputs.calibration_score.clamp(0.0, 1.0);
        let operator_component = inputs.operator.component();

        let components = ConfidenceComponents {
            source_reliability: reliability,
            sensor_health: health,
            freshness,
            corroboration,
            conflict,
            calibration_score: calibration,
            operator_feedback: operator_component,
        };

        // --- aggregate (weighted mean) then apply operator modulation -------
        let base = reliability * Weights::RELIABILITY
            + health * Weights::HEALTH
            + freshness * Weights::FRESHNESS
            + corroboration * Weights::CORROBORATION
            + conflict * Weights::CONFLICT
            + calibration * Weights::CALIBRATION;

        let score = match inputs.operator {
            OperatorInfluence::Neutral => base,
            OperatorInfluence::Confirmed => base + (1.0 - base) * 0.5,
            OperatorInfluence::Rejected => base * 0.25,
            OperatorInfluence::Adjusted(x) => (base + x * 0.5).clamp(0.0, 1.0),
        }
        .clamp(0.0, 1.0);

        // --- uncertainty band widens with weak corroboration / conflict / staleness
        let half = (0.05
            + 0.20 * (1.0 - corroboration)
            + 0.20 * (1.0 - conflict)
            + 0.10 * (1.0 - freshness))
            .clamp(0.0, 0.45);
        let uncertainty_band = [(score - half).max(0.0), (score + half).min(1.0)];

        // --- reason codes ---------------------------------------------------
        let mut reason_codes = Vec::new();
        if inputs.distinct_sources >= 2 {
            reason_codes.push(ReasonCode::MultiSourceCorroboration);
        } else {
            reason_codes.push(ReasonCode::SingleSourceOnly);
        }
        if inputs.freshest_age_ms as f64 > horizon * 0.5 {
            reason_codes.push(ReasonCode::StaleContribution);
        }
        if health < 0.6 && !inputs.source_health.is_empty() {
            reason_codes.push(ReasonCode::SensorDegraded);
        }
        if inputs.conflict_count > 0 {
            reason_codes.push(ReasonCode::ConflictDetected);
        }
        match inputs.operator {
            OperatorInfluence::Confirmed => reason_codes.push(ReasonCode::OperatorConfirmed),
            OperatorInfluence::Rejected => reason_codes.push(ReasonCode::OperatorRejected),
            _ => {}
        }
        if reliability < 0.5 {
            reason_codes.push(ReasonCode::LowSourceReliability);
        }
        if calibration >= 0.8 {
            reason_codes.push(ReasonCode::WellCalibrated);
        } else if calibration > 0.0 && calibration < 0.5 {
            reason_codes.push(ReasonCode::CalibrationDrift);
        }

        // --- calibration status --------------------------------------------
        let calibration_status = if calibration == 0.0 {
            CalibrationStatus::Uncalibrated
        } else if calibration >= 0.8 {
            CalibrationStatus::Calibrated
        } else if calibration >= 0.5 {
            CalibrationStatus::Drifting
        } else {
            CalibrationStatus::Uncalibrated
        };

        // --- degradation reason: present whenever a concrete negative signal
        // is dragging the score down (low score, conflict, or degraded sensor) -
        let degraded = score < 0.7
            || inputs.conflict_count > 0
            || (health < 0.6 && !inputs.source_health.is_empty());
        let degradation_reason = if degraded {
            Some(explain_degradation(&components, inputs))
        } else {
            None
        };

        ConfidenceVector {
            schema_version: SCHEMA_VERSION,
            computed_at: inputs.now,
            score,
            uncertainty_band,
            reason_codes,
            degradation_reason,
            calibration_status,
            components,
        }
    }

    fn name(&self) -> &'static str {
        "baseline-v0"
    }
}

/// Produce a short human-readable explanation of the weakest component.
fn explain_degradation(c: &ConfidenceComponents, inputs: &ConfidenceInputs) -> String {
    let candidates = [
        ("low source reliability", c.source_reliability),
        ("degraded sensor health", c.sensor_health),
        ("stale contributing data", c.freshness),
        ("single-source / weak corroboration", c.corroboration),
        ("unresolved conflicts", c.conflict),
        ("poor calibration history", c.calibration_score),
    ];
    let (label, value) = candidates
        .iter()
        .copied()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();

    if let OperatorInfluence::Rejected = inputs.operator {
        return "operator rejected the track".to_string();
    }
    format!("{label} (component {value:.2})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_source_emits_single_source_reason_code() {
        let scorer = BaselineConfidenceScorer::new();
        let cv = scorer.score(&ConfidenceInputs::single_source(Timestamp(1000), 0.8, 0.9));
        assert!(cv.reason_codes.contains(&ReasonCode::SingleSourceOnly));
        assert!(!cv.reason_codes.is_empty(), "must always include reason codes");
        assert!((0.0..=1.0).contains(&cv.score));
        assert!(cv.uncertainty_band[0] <= cv.score && cv.score <= cv.uncertainty_band[1]);
    }

    #[test]
    fn multi_source_beats_single_source() {
        let scorer = BaselineConfidenceScorer::new();
        let single = scorer.score(&ConfidenceInputs::single_source(Timestamp(0), 0.8, 0.9));

        let multi = scorer.score(&ConfidenceInputs {
            now: Timestamp(0),
            source_reliabilities: vec![0.8, 0.8, 0.8],
            source_health: vec![0.9, 0.9, 0.9],
            freshest_age_ms: 0,
            distinct_sources: 3,
            support_count: 3,
            conflict_count: 0,
            calibration_score: 0.9,
            operator: OperatorInfluence::Neutral,
            freshness_horizon_ms: 10_000,
        });

        assert!(multi.score > single.score);
        assert!(multi.reason_codes.contains(&ReasonCode::MultiSourceCorroboration));
    }

    #[test]
    fn conflict_lowers_score_and_is_flagged() {
        let scorer = BaselineConfidenceScorer::new();
        let cv = scorer.score(&ConfidenceInputs {
            now: Timestamp(0),
            source_reliabilities: vec![0.7, 0.7],
            source_health: vec![0.5, 0.9],
            freshest_age_ms: 0,
            distinct_sources: 2,
            support_count: 2,
            conflict_count: 3,
            calibration_score: 0.4,
            operator: OperatorInfluence::Neutral,
            freshness_horizon_ms: 10_000,
        });
        assert!(cv.reason_codes.contains(&ReasonCode::ConflictDetected));
        assert!(cv.reason_codes.contains(&ReasonCode::SensorDegraded));
        assert!(cv.degradation_reason.is_some());
    }

    #[test]
    fn deterministic() {
        let scorer = BaselineConfidenceScorer::new();
        let inputs = ConfidenceInputs::single_source(Timestamp(42), 0.6, 0.7);
        assert_eq!(scorer.score(&inputs).score, scorer.score(&inputs).score);
    }
}
