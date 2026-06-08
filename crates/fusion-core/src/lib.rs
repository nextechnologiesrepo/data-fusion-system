//! Fusion core.
//!
//! Two pieces, deliberately separated:
//!   * [`policy`] — the pluggable association/merge decision (`FusionPolicy`).
//!   * [`engine`] — the [`FusionEngine`] that adds staleness rejection,
//!     confidence scoring, append-only provenance, conflict preservation, and
//!     operator feedback around whatever policy is plugged in.
//!
//! v0 ships [`NearestNeighborPolicy`] + [`BaselineConfidenceScorer`]: simple,
//! deterministic, explainable, and easy to replace.

pub mod engine;
pub mod policy;

pub use engine::{FusionConfig, FusionEngine, FusionOutcome, RejectionReason};
pub use policy::{Association, FusionPolicy, GateConfig, NearestNeighborPolicy, TrackSnapshot};

// Re-export the companion engines so callers can build a working stack from one
// crate without naming every dependency.
pub use confidence_engine::{
    BaselineConfidenceScorer, ConfidenceInputs, ConfidenceScorer, OperatorInfluence,
};
pub use provenance_store::{
    InMemoryProvenanceStore, JsonlProvenanceStore, ProvenanceQuery, ProvenanceStore,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use shared_types::*;

    fn radar_obs(id: u64, source: &str, t: i64, pos: [f64; 3]) -> Observation {
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: ObservationId::seq(id),
            source_id: SourceId::new(source),
            source_kind: SourceKind::Radar,
            observed_at: Timestamp(t),
            received_at: Timestamp(t),
            payload: ObservationPayload::RadarTrack {
                range_m: 0.0,
                bearing_deg: 0.0,
                elevation_deg: 0.0,
                radial_velocity_mps: 0.0,
            },
            state: Some(StateEstimate::at(pos, 20.0)),
            measurement_confidence: 0.8,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }

    fn engine() -> (FusionEngine, Arc<InMemoryProvenanceStore>) {
        let store = Arc::new(InMemoryProvenanceStore::new());
        let eng = FusionEngine::new(
            Box::new(NearestNeighborPolicy::default()),
            Arc::new(BaselineConfidenceScorer::new()),
            store.clone(),
            FusionConfig::default(),
        );
        (eng, store)
    }

    #[test]
    fn ingest_creates_track_with_provenance_referencing_source() {
        let (mut eng, store) = engine();
        eng.register_source(Source::new(
            SourceId::new("radar-a"),
            SourceKind::Radar,
            "Radar A",
            0.9,
            Timestamp(0),
        ));

        let outcome = eng
            .process_observation(radar_obs(1, "radar-a", 1000, [0.0, 0.0, 0.0]))
            .unwrap();

        assert!(outcome.accepted);
        let track = outcome.track.unwrap();
        // Fused track references its contributing source observation.
        assert_eq!(track.contributing_observations, vec![ObservationId::seq(1)]);
        assert_eq!(track.contributing_sources, vec![SourceId::new("radar-a")]);
        // Provenance record was created and is linked from the track.
        assert_eq!(store.len(), 1);
        let prov = store.get(&track.provenance_ref).unwrap().unwrap();
        assert_eq!(prov.operation, ProvenanceOp::Created);
        // Confidence carries reason codes.
        assert!(!track.confidence.reason_codes.is_empty());
    }

    #[test]
    fn second_nearby_source_merges_and_confirms() {
        let (mut eng, _store) = engine();
        let o1 = eng
            .process_observation(radar_obs(1, "radar-a", 1000, [0.0, 0.0, 0.0]))
            .unwrap();
        let track_id = o1.track.unwrap().track_id;

        let o2 = eng
            .process_observation(radar_obs(2, "radar-b", 1100, [10.0, 0.0, 0.0]))
            .unwrap();
        let track = o2.track.unwrap();
        assert_eq!(track.track_id, track_id, "should merge into same track");
        assert_eq!(track.status, TrackStatus::Confirmed, "two sources confirm");
        assert_eq!(track.contributing_sources.len(), 2);
    }

    #[test]
    fn stale_observation_is_rejected() {
        let (mut eng, _store) = engine();
        // observed long before received → stale.
        let mut obs = radar_obs(1, "radar-a", 0, [0.0, 0.0, 0.0]);
        obs.received_at = Timestamp(1_000_000);
        let outcome = eng.process_observation(obs).unwrap();
        assert!(!outcome.accepted);
        assert!(matches!(outcome.rejection, Some(RejectionReason::Stale { .. })));
        assert_eq!(eng.stale_rejections(), 1);
    }
}
