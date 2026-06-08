//! The deterministic replay runner.

use std::sync::Arc;

use fusion_core::{
    BaselineConfidenceScorer, FusionConfig, FusionEngine, InMemoryProvenanceStore,
    NearestNeighborPolicy, ProvenanceStore,
};
use ingestion::{
    validate, EoIrGenerator, EwEmitterGenerator, PlatformStateGenerator, RadarGenerator,
    SyntheticTarget,
};
use shared_types::{
    EvaluationMetric, FabricError, FeedbackId, FusedTrack, Observation, ObservationId,
    OperatorFeedback, ProvenanceOp, ProvenanceRecord, ReplaySession, ReplayStatus, Result,
    SensorHealth, SessionId, Source, SourceId, Timestamp, TrackId, TrackStatus, SCHEMA_VERSION,
};

use crate::metrics::{compute, MetricInputs};
use crate::scenario::{EmitterKind, EmitterSpec, Scenario};

/// The output of one replay run.
#[derive(Debug, Clone)]
pub struct ReplayReport {
    pub session: ReplaySession,
    pub metrics: Vec<EvaluationMetric>,
    pub tracks: Vec<FusedTrack>,
    pub provenance: Vec<ProvenanceRecord>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ReplayHarness;

enum Emitter {
    Radar(RadarGenerator),
    Eoir(EoIrGenerator),
    Ew(EwEmitterGenerator),
    Platform(PlatformStateGenerator),
}

impl Emitter {
    fn emit(&mut self, now: Timestamp) -> Observation {
        match self {
            Emitter::Radar(g) => g.emit(now),
            Emitter::Eoir(g) => g.emit(now),
            Emitter::Ew(g) => g.emit(now),
            Emitter::Platform(g) => g.emit(now),
        }
    }
}

enum Action {
    Obs(Observation),
    Feedback(OperatorFeedback),
}

fn build_emitter(spec: &EmitterSpec, t0: Timestamp) -> Emitter {
    let target = SyntheticTarget::new(spec.target.start, spec.target.velocity);
    match spec.kind {
        EmitterKind::Radar => Emitter::Radar(RadarGenerator::new(&spec.source_id, target, t0, spec.seed)),
        EmitterKind::Eoir => Emitter::Eoir(EoIrGenerator::new(
            &spec.source_id,
            target,
            spec.classification.as_deref().unwrap_or("unknown"),
            t0,
            spec.seed,
        )),
        EmitterKind::EwSigint => Emitter::Ew(EwEmitterGenerator::new(
            &spec.source_id,
            target,
            spec.frequency_mhz.unwrap_or(9400.0),
            t0,
            spec.seed,
        )),
        EmitterKind::Platform => {
            Emitter::Platform(PlatformStateGenerator::new(&spec.source_id, target, t0, spec.seed))
        }
    }
}

impl ReplayHarness {
    pub fn new() -> Self {
        ReplayHarness
    }

    pub fn run(&self, scenario: &Scenario) -> Result<ReplayReport> {
        let t0 = Timestamp(scenario.timeline.start_ms);
        let t_end = Timestamp(scenario.timeline.end_ms);

        // --- engine assembly (fresh per run → deterministic) ----------------
        let mut config = FusionConfig::default();
        let c = &scenario.config;
        if let Some(v) = c.staleness_limit_ms {
            config.staleness_limit_ms = v;
        }
        if let Some(v) = c.freshness_horizon_ms {
            config.freshness_horizon_ms = v;
        }
        if let Some(v) = c.confirm_min_sources {
            config.confirm_min_sources = v;
        }
        if let Some(v) = c.confirm_min_observations {
            config.confirm_min_observations = v;
        }
        if let Some(v) = c.default_calibration_score {
            config.default_calibration_score = v;
        }

        let store = Arc::new(InMemoryProvenanceStore::new());
        let mut engine = FusionEngine::new(
            Box::new(NearestNeighborPolicy::default()),
            Arc::new(BaselineConfidenceScorer::new()),
            store.clone(),
            config,
        );

        let session_id = SessionId::new(format!("ses-{}", scenario.name));

        // --- registry -------------------------------------------------------
        for s in &scenario.sources {
            engine.register_source(Source::new(
                SourceId::new(&s.source_id),
                s.kind,
                s.name.clone().unwrap_or_else(|| s.source_id.clone()),
                s.reliability,
                t0,
            ));
        }
        for h in &scenario.health {
            engine.report_health(SensorHealth {
                schema_version: SCHEMA_VERSION,
                source_id: SourceId::new(&h.source_id),
                reported_at: t0,
                status: h.status,
                health_score: h.health_score,
                detail: h.detail.clone(),
            });
        }

        // --- materialize the event log --------------------------------------
        let mut emitters: Vec<Emitter> =
            scenario.emitters.iter().map(|s| build_emitter(s, t0)).collect();

        let mut actions: Vec<(i64, Action)> = Vec::new();
        let step = scenario.timeline.step_ms.max(1);
        let mut t = scenario.timeline.start_ms;
        while t <= scenario.timeline.end_ms {
            let now = Timestamp(t);
            for (idx, em) in emitters.iter_mut().enumerate() {
                let mut obs = em.emit(now);
                let lag = scenario.emitters[idx].observed_lag_ms;
                if lag != 0 {
                    obs.observed_at = Timestamp(t - lag);
                }
                actions.push((t, Action::Obs(obs)));
            }
            t += step;
        }
        for (i, f) in scenario.feedback.iter().enumerate() {
            actions.push((
                f.at_ms,
                Action::Feedback(OperatorFeedback {
                    schema_version: SCHEMA_VERSION,
                    feedback_id: FeedbackId::seq(i as u64 + 1),
                    track_id: TrackId::new(&f.track),
                    operator_id: f.operator.clone(),
                    submitted_at: Timestamp(f.at_ms),
                    verdict: f.verdict,
                    note: f.note.clone(),
                    confidence_adjustment: f.confidence_adjustment,
                }),
            ));
        }
        // Stable sort keeps emit order within a timestamp → deterministic.
        actions.sort_by_key(|(at, _)| *at);

        // --- run ------------------------------------------------------------
        let mut accepted = 0usize;
        let mut operator_overrides = 0usize;
        let mut event_count = 0u64;
        for (_at, action) in actions {
            event_count += 1;
            match action {
                Action::Obs(o) => {
                    if validate(&o).is_err() {
                        continue;
                    }
                    let outcome = engine.process_observation(o)?;
                    if outcome.accepted {
                        accepted += 1;
                    }
                }
                Action::Feedback(fb) => match engine.apply_operator_feedback(fb) {
                    Ok(_) => operator_overrides += 1,
                    Err(FabricError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                },
            }
        }

        // --- score ----------------------------------------------------------
        let tracks = engine.all_tracks();
        let confirmed: Vec<&FusedTrack> =
            tracks.iter().filter(|t| t.status == TrackStatus::Confirmed).collect();
        let mean_conf = if confirmed.is_empty() {
            0.0
        } else {
            confirmed.iter().map(|t| t.confidence.score).sum::<f64>() / confirmed.len() as f64
        };
        let latencies: Vec<i64> = confirmed
            .iter()
            .filter_map(|t| engine.confirmation_latency_ms(&t.track_id))
            .collect();
        let total_conflicts: usize = tracks.iter().map(|t| t.conflicts.len()).sum();
        let provenance = store.all()?;
        let completeness = provenance_completeness(&store, &tracks)?;

        let metrics = compute(&MetricInputs {
            session_id: session_id.clone(),
            computed_at: t_end,
            ground_truth_tracks: scenario.ground_truth_tracks,
            confirmed_tracks: confirmed.len(),
            mean_confirmed_confidence: mean_conf,
            confirmation_latencies_ms: latencies,
            stale_rejections: engine.stale_rejections(),
            total_conflicts,
            accepted_observations: accepted,
            operator_overrides,
            provenance_completeness: completeness,
        });

        let session = ReplaySession {
            schema_version: SCHEMA_VERSION,
            session_id,
            scenario_name: scenario.name.clone(),
            started_at: t0,
            finished_at: Some(t_end),
            deterministic: scenario.deterministic,
            seed: scenario.seed,
            status: ReplayStatus::Completed,
            event_count,
        };

        Ok(ReplayReport {
            session,
            metrics,
            tracks,
            provenance,
        })
    }
}

/// Fraction of tracks whose provenance chain starts with a `Created` record and
/// covers every contributing observation.
fn provenance_completeness(
    store: &InMemoryProvenanceStore,
    tracks: &[FusedTrack],
) -> Result<f64> {
    if tracks.is_empty() {
        return Ok(1.0);
    }
    let mut complete = 0usize;
    for t in tracks {
        let chain = store.chain_for_track(&t.track_id)?;
        let has_created = chain
            .first()
            .map(|r| r.operation == ProvenanceOp::Created)
            .unwrap_or(false);

        let mut union: Vec<ObservationId> = Vec::new();
        for r in &chain {
            for o in &r.contributing_observations {
                if !union.contains(o) {
                    union.push(o.clone());
                }
            }
        }
        let covers = t.contributing_observations.iter().all(|o| union.contains(o));
        if has_created && covers {
            complete += 1;
        }
    }
    Ok(complete as f64 / tracks.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    const DEMO: &str = include_str!("../../../sim/scenarios/scenario-01.json");

    #[test]
    fn demo_scenario_parses() {
        let _scenario: Scenario = serde_json::from_str(DEMO).expect("scenario JSON must parse");
    }

    #[test]
    fn replay_is_deterministic() {
        let scenario: Scenario = serde_json::from_str(DEMO).unwrap();
        let r1 = ReplayHarness::new().run(&scenario).unwrap();
        let r2 = ReplayHarness::new().run(&scenario).unwrap();
        assert_eq!(r1.metrics, r2.metrics, "metrics must be identical across runs");
        assert_eq!(r1.tracks, r2.tracks, "tracks must be identical across runs");
        assert!(
            r1.tracks.iter().any(|t| t.status == TrackStatus::Confirmed),
            "scenario should confirm at least one track"
        );
    }

    #[test]
    fn replay_produces_all_eight_metrics() {
        let scenario: Scenario = serde_json::from_str(DEMO).unwrap();
        let report = ReplayHarness::new().run(&scenario).unwrap();
        assert_eq!(report.metrics.len(), 8);
    }
}
