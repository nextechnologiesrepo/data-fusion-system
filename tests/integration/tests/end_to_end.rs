//! End-to-end proofs for the first pass (requirement 9):
//!   * observations can be ingested
//!   * provenance records are created
//!   * a fused track references its source observations
//!   * confidence output includes reason codes
//!   * replay can run deterministically

use std::sync::Arc;

use fusion_core::{
    BaselineConfidenceScorer, FusionConfig, FusionEngine, InMemoryProvenanceStore,
    NearestNeighborPolicy, ProvenanceStore,
};
use ingestion::validate;
use replay::{ReplayHarness, Scenario};
use shared_types::{MetricKind, Observation, TrackStatus};

const FIXTURE: &str = include_str!("../../fixtures/observations.json");
const SCENARIO: &str = include_str!("../../../sim/scenarios/scenario-01.json");

fn assemble() -> (FusionEngine, Arc<InMemoryProvenanceStore>) {
    let store = Arc::new(InMemoryProvenanceStore::new());
    let engine = FusionEngine::new(
        Box::new(NearestNeighborPolicy::default()),
        Arc::new(BaselineConfidenceScorer::new()),
        store.clone(),
        FusionConfig::default(),
    );
    (engine, store)
}

#[test]
fn ingest_creates_provenance_and_tracks_that_reference_their_observations() {
    let (mut engine, store) = assemble();
    let observations: Vec<Observation> =
        serde_json::from_str(FIXTURE).expect("fixture observations parse");
    assert_eq!(observations.len(), 2);

    // observations can be ingested
    let mut obs_ids = Vec::new();
    for obs in observations {
        validate(&obs).expect("fixture observation is valid");
        obs_ids.push(obs.observation_id.clone());
        let outcome = engine.process_observation(obs).expect("ingest succeeds");
        assert!(outcome.accepted, "observation accepted");
    }

    // provenance records are created
    let records = store.all().unwrap();
    assert!(!records.is_empty(), "provenance records were created");

    // a fused track references its source observations
    let tracks = engine.all_tracks();
    assert_eq!(tracks.len(), 1, "two nearby observations merge into one track");
    let track = &tracks[0];
    for id in &obs_ids {
        assert!(
            track.contributing_observations.contains(id),
            "fused track references contributing observation {id}"
        );
    }
    assert_eq!(track.contributing_sources.len(), 2, "two distinct sources");
    assert_eq!(track.status, TrackStatus::Confirmed, "two sources confirm");

    // the track's provenance reference resolves in the store
    assert!(store.get(&track.provenance_ref).unwrap().is_some());

    // confidence output includes reason codes
    assert!(
        !track.confidence.reason_codes.is_empty(),
        "confidence output carries reason codes"
    );
}

#[test]
fn replay_runs_deterministically() {
    let scenario: Scenario = serde_json::from_str(SCENARIO).expect("scenario parses");

    let r1 = ReplayHarness::new().run(&scenario).unwrap();
    let r2 = ReplayHarness::new().run(&scenario).unwrap();

    // replay can run deterministically
    assert_eq!(r1.metrics, r2.metrics, "metrics identical across runs");
    assert_eq!(r1.tracks, r2.tracks, "tracks identical across runs");

    // all eight evaluation metrics are produced
    assert_eq!(r1.metrics.len(), 8);
    assert!(
        r1.tracks.iter().any(|t| t.status == TrackStatus::Confirmed),
        "scenario confirms at least one track"
    );

    let value_of = |k: MetricKind| {
        r1.metrics
            .iter()
            .find(|m| m.metric == k)
            .unwrap_or_else(|| panic!("metric {k:?} present"))
            .value
    };
    assert_eq!(
        value_of(MetricKind::ProvenanceCompleteness),
        1.0,
        "every fused track is fully traceable"
    );
    assert_eq!(
        value_of(MetricKind::StaleDataRejectionCount),
        7.0,
        "the laggy source's reports are rejected as stale"
    );
}
