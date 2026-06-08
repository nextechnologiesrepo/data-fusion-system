//! Fusion engine — orchestration.
//!
//! The engine owns the assurance plumbing the [`FusionPolicy`] deliberately does
//! not: timestamp handling, staleness rejection, confidence scoring, append-only
//! provenance, conflict preservation, and operator-feedback application. It is
//! single-threaded by contract (callers wrap it in a mutex); processing a fixed
//! sequence of observations therefore yields identical IDs and outputs every run.

use std::collections::HashMap;
use std::sync::Arc;

use confidence_engine::{ConfidenceInputs, ConfidenceScorer, OperatorInfluence};
use provenance_store::ProvenanceStore;
use shared_types::{
    ConflictRecord, CueSeverity, FabricError, FeedbackVerdict, FusedTrack, Observation,
    OperatorFeedback, ProvenanceId, ProvenanceOp, ProvenanceRecord, RecommendationCue, Result,
    SensorHealth, Source, SourceId, Timestamp, TrackId, TrackStatus, CueId, SCHEMA_VERSION,
};

use crate::policy::{Association, FusionPolicy, TrackSnapshot};

/// Engine tunables.
#[derive(Debug, Clone, Copy)]
pub struct FusionConfig {
    /// Observations whose `observed_at` is older than this (relative to their
    /// `received_at`) are rejected before fusion.
    pub staleness_limit_ms: i64,
    /// Passed to the confidence engine as the freshness horizon.
    pub freshness_horizon_ms: i64,
    /// Distinct sources needed to promote a track to `Confirmed`.
    pub confirm_min_sources: usize,
    /// Supporting observations needed to promote a track to `Confirmed`.
    pub confirm_min_observations: usize,
    /// Reliability assumed for sources that were never registered.
    pub default_source_reliability: f64,
    /// Engine-wide calibration score fed to the confidence engine in v0.
    pub default_calibration_score: f64,
}

impl Default for FusionConfig {
    fn default() -> Self {
        FusionConfig {
            staleness_limit_ms: 5_000,
            freshness_horizon_ms: 10_000,
            confirm_min_sources: 2,
            confirm_min_observations: 3,
            default_source_reliability: 0.5,
            default_calibration_score: 0.0,
        }
    }
}

/// Why an observation was not fused.
#[derive(Debug, Clone, PartialEq)]
pub enum RejectionReason {
    Stale { age_ms: i64, limit_ms: i64 },
    SignatureInvalid,
}

/// The result of feeding one observation (or feedback) to the engine.
#[derive(Debug, Clone)]
pub struct FusionOutcome {
    pub accepted: bool,
    pub rejection: Option<RejectionReason>,
    pub operation: Option<ProvenanceOp>,
    pub track: Option<FusedTrack>,
    pub provenance: Option<ProvenanceRecord>,
    pub cue: Option<RecommendationCue>,
    /// Set for accepted-but-not-fused observations (e.g. non-kinematic).
    pub note: Option<String>,
}

impl FusionOutcome {
    fn rejected(reason: RejectionReason) -> Self {
        FusionOutcome {
            accepted: false,
            rejection: Some(reason),
            operation: None,
            track: None,
            provenance: None,
            cue: None,
            note: None,
        }
    }
}

/// Internal bookkeeping for a live track.
struct TrackEntry {
    track: FusedTrack,
    supporting: Vec<shared_types::ObservationId>,
    conflicting: Vec<shared_types::ObservationId>,
    sources: Vec<SourceId>, // unique, insertion-ordered
    first_obs_at: Timestamp,
    confirmed_at: Option<Timestamp>,
    last_provenance: ProvenanceId,
}

pub struct FusionEngine {
    policy: Box<dyn FusionPolicy>,
    scorer: Arc<dyn ConfidenceScorer>,
    provenance: Arc<dyn ProvenanceStore>,
    config: FusionConfig,

    sources: HashMap<SourceId, Source>,
    health: HashMap<SourceId, SensorHealth>,
    observations: HashMap<shared_types::ObservationId, Observation>,
    tracks: HashMap<TrackId, TrackEntry>,
    operator: HashMap<TrackId, OperatorInfluence>,

    track_seq: u64,
    prov_seq: u64,
    cue_seq: u64,

    stale_rejections: u64,
}

impl FusionEngine {
    pub fn new(
        policy: Box<dyn FusionPolicy>,
        scorer: Arc<dyn ConfidenceScorer>,
        provenance: Arc<dyn ProvenanceStore>,
        config: FusionConfig,
    ) -> Self {
        FusionEngine {
            policy,
            scorer,
            provenance,
            config,
            sources: HashMap::new(),
            health: HashMap::new(),
            observations: HashMap::new(),
            tracks: HashMap::new(),
            operator: HashMap::new(),
            track_seq: 0,
            prov_seq: 0,
            cue_seq: 0,
            stale_rejections: 0,
        }
    }

    // --- registry -----------------------------------------------------------

    pub fn register_source(&mut self, source: Source) {
        self.sources.insert(source.source_id.clone(), source);
    }

    pub fn report_health(&mut self, health: SensorHealth) {
        self.health.insert(health.source_id.clone(), health);
    }

    pub fn list_sources(&self) -> Vec<Source> {
        let mut v: Vec<Source> = self.sources.values().cloned().collect();
        v.sort_by(|a, b| a.source_id.cmp(&b.source_id));
        v
    }

    pub fn get_track(&self, id: &TrackId) -> Option<FusedTrack> {
        self.tracks.get(id).map(|e| e.track.clone())
    }

    pub fn all_tracks(&self) -> Vec<FusedTrack> {
        let mut v: Vec<FusedTrack> = self.tracks.values().map(|e| e.track.clone()).collect();
        v.sort_by(|a, b| a.track_id.cmp(&b.track_id));
        v
    }

    pub fn stale_rejections(&self) -> u64 {
        self.stale_rejections
    }

    /// Confirmation latency for a track, if it has been confirmed.
    pub fn confirmation_latency_ms(&self, id: &TrackId) -> Option<i64> {
        let e = self.tracks.get(id)?;
        e.confirmed_at.map(|c| c.millis() - e.first_obs_at.millis())
    }

    // --- main path ----------------------------------------------------------

    /// Ingest and fuse a single observation.
    pub fn process_observation(&mut self, obs: Observation) -> Result<FusionOutcome> {
        // Signed-event check (v0 placeholder: none-v0 always verifies).
        if !obs.signature.verify(&[]) {
            return Ok(FusionOutcome::rejected(RejectionReason::SignatureInvalid));
        }

        // Timestamp normalization is trivial in v0 (already epoch-millis); "now"
        // is the observation's receive time so the path stays data-driven.
        let now = obs.received_at;
        let age = obs.observed_at.age_ms(now);
        if age > self.config.staleness_limit_ms {
            self.stale_rejections += 1;
            return Ok(FusionOutcome::rejected(RejectionReason::Stale {
                age_ms: age,
                limit_ms: self.config.staleness_limit_ms,
            }));
        }

        // Auto-register a source on first sighting so `/sources` reflects live
        // feeds. A source seen via ingestion but never formally registered gets
        // the configured default reliability until told otherwise.
        if !self.sources.contains_key(&obs.source_id) {
            self.register_source(Source::new(
                obs.source_id.clone(),
                obs.source_kind,
                obs.source_id.as_str(),
                self.config.default_source_reliability,
                now,
            ));
        }

        // Non-kinematic observations are recorded for audit but not associated.
        let Some(_state) = obs.state else {
            self.observations.insert(obs.observation_id.clone(), obs.clone());
            return Ok(FusionOutcome {
                accepted: true,
                rejection: None,
                operation: None,
                track: None,
                provenance: None,
                cue: None,
                note: Some("non-kinematic observation recorded for audit only".to_string()),
            });
        };

        let snapshots: Vec<TrackSnapshot> = self
            .tracks
            .values()
            .filter(|e| e.track.status != TrackStatus::Dropped)
            .map(|e| TrackSnapshot {
                track_id: e.track.track_id.clone(),
                state: e.track.state,
            })
            .collect();

        let association = self.policy.associate(&obs, &snapshots);
        self.observations.insert(obs.observation_id.clone(), obs.clone());

        match association {
            Association::New => self.create_track(obs, now),
            Association::Merge { track_id, .. } => self.merge_into(track_id, obs, now),
            Association::Conflict {
                track_id,
                divergence_m,
            } => self.preserve_conflict(track_id, obs, now, divergence_m),
        }
    }

    // --- association handlers ----------------------------------------------

    fn create_track(&mut self, obs: Observation, now: Timestamp) -> Result<FusionOutcome> {
        self.track_seq += 1;
        let track_id = TrackId::seq(self.track_seq);
        let state = obs.state.expect("kinematic observation");

        let mut entry = TrackEntry {
            track: FusedTrack {
                schema_version: SCHEMA_VERSION,
                track_id: track_id.clone(),
                version: 0, // filled by publish()
                created_at: now,
                updated_at: now,
                state,
                confidence: self.scorer.score(&ConfidenceInputs::single_source(now, 0.0, 0.0)),
                provenance_ref: ProvenanceId::new("pending"),
                contributing_observations: vec![obs.observation_id.clone()],
                contributing_sources: vec![obs.source_id.clone()],
                conflicts: Vec::new(),
                status: TrackStatus::Tentative,
            },
            supporting: vec![obs.observation_id.clone()],
            conflicting: Vec::new(),
            sources: vec![obs.source_id.clone()],
            first_obs_at: obs.observed_at,
            confirmed_at: None,
            last_provenance: ProvenanceId::new("pending"),
        };

        let outcome = self.publish(&mut entry, now, ProvenanceOp::Created, None, &obs)?;
        self.tracks.insert(track_id, entry);
        Ok(outcome)
    }

    fn merge_into(
        &mut self,
        track_id: TrackId,
        obs: Observation,
        now: Timestamp,
    ) -> Result<FusionOutcome> {
        let mut entry = self
            .tracks
            .remove(&track_id)
            .ok_or_else(|| FabricError::NotFound(format!("track {track_id}")))?;

        let incoming = obs.state.expect("kinematic observation");
        let before = entry.track.confidence.score;
        entry.track.state = self.policy.merge_state(&entry.track.state, &incoming);
        entry.supporting.push(obs.observation_id.clone());
        if !entry.sources.contains(&obs.source_id) {
            entry.sources.push(obs.source_id.clone());
        }

        let outcome = self.publish(&mut entry, now, ProvenanceOp::Merged, Some(before), &obs)?;
        self.tracks.insert(track_id, entry);
        Ok(outcome)
    }

    fn preserve_conflict(
        &mut self,
        track_id: TrackId,
        obs: Observation,
        now: Timestamp,
        divergence_m: f64,
    ) -> Result<FusionOutcome> {
        let mut entry = self
            .tracks
            .remove(&track_id)
            .ok_or_else(|| FabricError::NotFound(format!("track {track_id}")))?;

        let before = entry.track.confidence.score;
        entry.conflicting.push(obs.observation_id.clone());
        entry.track.conflicts.push(ConflictRecord {
            observation_id: obs.observation_id.clone(),
            source_id: obs.source_id.clone(),
            reason: "observation outside merge gate but within conflict gate".to_string(),
            divergence_m,
        });
        // A conflicting source still counts toward "sources touching this track".
        if !entry.sources.contains(&obs.source_id) {
            entry.sources.push(obs.source_id.clone());
        }

        let outcome = self.publish(
            &mut entry,
            now,
            ProvenanceOp::ConflictPreserved,
            Some(before),
            &obs,
        )?;
        self.tracks.insert(track_id, entry);
        Ok(outcome)
    }

    // --- shared publish step: score, version, provenance, cue ---------------

    fn publish(
        &mut self,
        entry: &mut TrackEntry,
        now: Timestamp,
        operation: ProvenanceOp,
        confidence_before: Option<f64>,
        trigger: &Observation,
    ) -> Result<FusionOutcome> {
        // 1. (re)score confidence from the current membership.
        let inputs = self.build_inputs(entry, now);
        let confidence = self.scorer.score(&inputs);

        // 2. status promotion.
        let distinct = entry.sources.len();
        let support = entry.supporting.len();
        let newly_confirmed = entry.confirmed_at.is_none()
            && (distinct >= self.config.confirm_min_sources
                || support >= self.config.confirm_min_observations);
        if newly_confirmed {
            entry.confirmed_at = Some(now);
        }
        let status = if entry.confirmed_at.is_some() {
            TrackStatus::Confirmed
        } else {
            TrackStatus::Tentative
        };

        // 3. append provenance.
        self.prov_seq += 1;
        let provenance_id = ProvenanceId::seq(self.prov_seq);
        let prev = if operation == ProvenanceOp::Created {
            None
        } else {
            Some(entry.last_provenance.clone())
        };
        let version = entry.track.version + 1;
        let record = ProvenanceRecord {
            schema_version: SCHEMA_VERSION,
            provenance_id: provenance_id.clone(),
            track_id: entry.track.track_id.clone(),
            fused_version: version,
            created_at: now,
            operation,
            contributing_observations: vec![trigger.observation_id.clone()],
            contributing_sources: vec![trigger.source_id.clone()],
            confidence_before,
            confidence_after: confidence.score,
            notes: format!("{operation:?} via {}", trigger.source_id),
            prev_provenance_id: prev,
        };
        self.provenance.append(record.clone())?;
        entry.last_provenance = provenance_id.clone();

        // 4. update the published track.
        entry.track.version = version;
        entry.track.updated_at = now;
        entry.track.confidence = confidence.clone();
        entry.track.provenance_ref = provenance_id;
        entry.track.contributing_observations = entry.supporting.clone();
        entry.track.contributing_sources = entry.sources.clone();
        entry.track.status = status;

        // 5. decision-support cue.
        let cue = self.build_cue(entry, now, newly_confirmed);

        Ok(FusionOutcome {
            accepted: true,
            rejection: None,
            operation: Some(operation),
            track: Some(entry.track.clone()),
            provenance: Some(record),
            cue,
            note: None,
        })
    }

    fn build_inputs(&self, entry: &TrackEntry, now: Timestamp) -> ConfidenceInputs {
        let reliabilities = entry
            .sources
            .iter()
            .map(|s| {
                self.sources
                    .get(s)
                    .map(|src| src.reliability)
                    .unwrap_or(self.config.default_source_reliability)
            })
            .collect();
        let health = entry
            .sources
            .iter()
            .map(|s| self.health.get(s).map(|h| h.health_score).unwrap_or(1.0))
            .collect();

        let freshest_age_ms = entry
            .supporting
            .iter()
            .filter_map(|id| self.observations.get(id))
            .map(|o| o.observed_at.age_ms(now))
            .min()
            .unwrap_or(0);

        let operator = self
            .operator
            .get(&entry.track.track_id)
            .copied()
            .unwrap_or(OperatorInfluence::Neutral);

        ConfidenceInputs {
            now,
            source_reliabilities: reliabilities,
            source_health: health,
            freshest_age_ms,
            distinct_sources: entry.sources.len(),
            support_count: entry.supporting.len(),
            conflict_count: entry.conflicting.len(),
            calibration_score: self.config.default_calibration_score,
            operator,
            freshness_horizon_ms: self.config.freshness_horizon_ms,
        }
    }

    fn build_cue(
        &mut self,
        entry: &TrackEntry,
        now: Timestamp,
        newly_confirmed: bool,
    ) -> Option<RecommendationCue> {
        let (severity, message, action) = if !entry.conflicting.is_empty() {
            (
                CueSeverity::Warning,
                format!(
                    "{} conflicting observation(s) preserved on this track",
                    entry.conflicting.len()
                ),
                "Review conflicts before acting on this track".to_string(),
            )
        } else if entry.track.status == TrackStatus::Confirmed && entry.track.confidence.score < 0.5
        {
            (
                CueSeverity::Caution,
                "Track confirmed at low confidence".to_string(),
                "Corroborate with another source before acting".to_string(),
            )
        } else if newly_confirmed {
            (
                CueSeverity::Info,
                "Track confirmed".to_string(),
                "No action required".to_string(),
            )
        } else {
            return None;
        };

        self.cue_seq += 1;
        Some(RecommendationCue {
            schema_version: SCHEMA_VERSION,
            cue_id: CueId::seq(self.cue_seq),
            track_id: entry.track.track_id.clone(),
            created_at: now,
            severity,
            message,
            confidence_ref: entry.track.confidence.score,
            recommended_action: action,
        })
    }

    // --- operator feedback --------------------------------------------------

    /// Apply operator feedback: update influence, re-score, and record an
    /// append-only `OperatorOverride` provenance entry.
    pub fn apply_operator_feedback(&mut self, fb: OperatorFeedback) -> Result<FusionOutcome> {
        let now = fb.submitted_at;
        let influence = match fb.verdict {
            FeedbackVerdict::ConfirmTrack => OperatorInfluence::Confirmed,
            FeedbackVerdict::RejectTrack => OperatorInfluence::Rejected,
            FeedbackVerdict::AdjustConfidence => {
                OperatorInfluence::Adjusted(fb.confidence_adjustment.unwrap_or(0.0))
            }
            FeedbackVerdict::Reclassify => OperatorInfluence::Neutral,
        };
        self.operator.insert(fb.track_id.clone(), influence);

        let mut entry = self
            .tracks
            .remove(&fb.track_id)
            .ok_or_else(|| FabricError::NotFound(format!("track {}", fb.track_id)))?;

        let before = entry.track.confidence.score;
        let inputs = self.build_inputs(&entry, now);
        let confidence = self.scorer.score(&inputs);

        if fb.verdict == FeedbackVerdict::RejectTrack {
            entry.track.status = TrackStatus::Dropped;
        }

        self.prov_seq += 1;
        let provenance_id = ProvenanceId::seq(self.prov_seq);
        let version = entry.track.version + 1;
        let record = ProvenanceRecord {
            schema_version: SCHEMA_VERSION,
            provenance_id: provenance_id.clone(),
            track_id: entry.track.track_id.clone(),
            fused_version: version,
            created_at: now,
            operation: ProvenanceOp::OperatorOverride,
            contributing_observations: Vec::new(),
            contributing_sources: Vec::new(),
            confidence_before: Some(before),
            confidence_after: confidence.score,
            notes: format!("operator {} verdict {:?}", fb.operator_id, fb.verdict),
            prev_provenance_id: Some(entry.last_provenance.clone()),
        };
        self.provenance.append(record.clone())?;
        entry.last_provenance = provenance_id.clone();

        entry.track.version = version;
        entry.track.updated_at = now;
        entry.track.confidence = confidence;
        entry.track.provenance_ref = provenance_id;

        let track = entry.track.clone();
        self.tracks.insert(fb.track_id.clone(), entry);

        Ok(FusionOutcome {
            accepted: true,
            rejection: None,
            operation: Some(ProvenanceOp::OperatorOverride),
            track: Some(track),
            provenance: Some(record),
            cue: None,
            note: None,
        })
    }
}
