//! Provenance fabric.
//!
//! Every change to a fused track writes one [`ProvenanceRecord`]. Records are
//! **append-only**: a store rejects re-use of an existing `provenance_id` and
//! never mutates a stored record. Reading a track's records oldest→newest
//! reconstructs exactly how the track came to be.
//!
//! The store is split into two traits:
//!   * [`ProvenanceStore`] — the minimal append/read primitives a backend must
//!     implement (in-memory, file-backed JSONL today; SQLite later).
//!   * [`ProvenanceQuery`] — the operator-facing questions, implemented once on
//!     top of the primitives so every backend answers them identically.

mod memory;
mod jsonl;

pub use jsonl::JsonlProvenanceStore;
pub use memory::InMemoryProvenanceStore;

use shared_types::{
    FabricError, ObservationId, ProvenanceId, ProvenanceOp, ProvenanceRecord, Result, SourceId,
    TrackId,
};

/// Append-only storage primitives. Backends implement only these five methods.
pub trait ProvenanceStore: Send + Sync {
    /// Append a record. Errors if `provenance_id` already exists (append-only)
    /// or if `prev_provenance_id` references a record that is not present.
    fn append(&self, record: ProvenanceRecord) -> Result<()>;

    fn get(&self, id: &ProvenanceId) -> Result<Option<ProvenanceRecord>>;

    /// All records for a track, ordered oldest → newest by `fused_version`.
    fn chain_for_track(&self, track: &TrackId) -> Result<Vec<ProvenanceRecord>>;

    /// The newest record for a track, if any.
    fn latest_for_track(&self, track: &TrackId) -> Result<Option<ProvenanceRecord>> {
        Ok(self.chain_for_track(track)?.pop())
    }

    /// Every record in the store (for replay export / audit dumps).
    fn all(&self) -> Result<Vec<ProvenanceRecord>>;
}

/// The describe-the-track-back-to-its-sources answer for one source step.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfidenceImpact {
    pub provenance_id: ProvenanceId,
    pub operation: ProvenanceOp,
    pub sources: Vec<SourceId>,
    /// Negative when this step lowered confidence.
    pub confidence_delta: f64,
}

/// What changed between two fused versions of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenanceDiff {
    pub track_id: TrackId,
    pub from_version: u64,
    pub to_version: u64,
    pub operation: ProvenanceOp,
    pub added_observations: Vec<ObservationId>,
    pub added_sources: Vec<SourceId>,
    pub confidence_delta: f64,
    pub notes: String,
}

/// Operator-facing provenance queries, available on any [`ProvenanceStore`].
pub trait ProvenanceQuery: ProvenanceStore {
    /// "Why does this fused track exist?" — the originating `Created` record.
    fn why_does_track_exist(&self, track: &TrackId) -> Result<ProvenanceRecord> {
        self.chain_for_track(track)?
            .into_iter()
            .find(|r| r.operation == ProvenanceOp::Created)
            .ok_or_else(|| FabricError::NotFound(format!("no creation record for {track}")))
    }

    /// "Which observations contributed to it?" — de-duplicated across the chain,
    /// in first-seen order.
    fn contributing_observations(&self, track: &TrackId) -> Result<Vec<ObservationId>> {
        let mut seen = Vec::new();
        for record in self.chain_for_track(track)? {
            for obs in record.contributing_observations {
                if !seen.contains(&obs) {
                    seen.push(obs);
                }
            }
        }
        Ok(seen)
    }

    /// "Which source lowered confidence?" — every step whose confidence fell,
    /// newest first, attributed to the sources active at that step.
    fn sources_that_lowered_confidence(&self, track: &TrackId) -> Result<Vec<ConfidenceImpact>> {
        let mut out = Vec::new();
        for record in self.chain_for_track(track)? {
            let before = record.confidence_before.unwrap_or(record.confidence_after);
            let delta = record.confidence_after - before;
            if delta < 0.0 {
                out.push(ConfidenceImpact {
                    provenance_id: record.provenance_id,
                    operation: record.operation,
                    sources: record.contributing_sources,
                    confidence_delta: delta,
                });
            }
        }
        out.reverse();
        Ok(out)
    }

    /// "What changed since the previous fused version?" — diff of the two
    /// newest records. `None` if the track has only ever had one version.
    fn changed_since_previous(&self, track: &TrackId) -> Result<Option<ProvenanceDiff>> {
        let chain = self.chain_for_track(track)?;
        let n = chain.len();
        if n < 2 {
            return Ok(None);
        }
        let prev = &chain[n - 2];
        let curr = &chain[n - 1];

        let added_observations = curr
            .contributing_observations
            .iter()
            .filter(|o| !prev.contributing_observations.contains(o))
            .cloned()
            .collect();
        let added_sources = curr
            .contributing_sources
            .iter()
            .filter(|s| !prev.contributing_sources.contains(s))
            .cloned()
            .collect();

        Ok(Some(ProvenanceDiff {
            track_id: track.clone(),
            from_version: prev.fused_version,
            to_version: curr.fused_version,
            operation: curr.operation,
            added_observations,
            added_sources,
            confidence_delta: curr.confidence_after - prev.confidence_after,
            notes: curr.notes.clone(),
        }))
    }
}

// Every store gets the queries for free.
impl<T: ProvenanceStore + ?Sized> ProvenanceQuery for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{ObservationId, SourceId, Timestamp, SCHEMA_VERSION};

    #[allow(clippy::too_many_arguments)]
    fn rec(
        pid: &str,
        ver: u64,
        op: ProvenanceOp,
        before: Option<f64>,
        after: f64,
        prev: Option<&str>,
        obs: &[&str],
        src: &[&str],
    ) -> ProvenanceRecord {
        ProvenanceRecord {
            schema_version: SCHEMA_VERSION,
            provenance_id: ProvenanceId::new(pid),
            track_id: TrackId::new("trk-1"),
            fused_version: ver,
            created_at: Timestamp(ver as i64),
            operation: op,
            contributing_observations: obs.iter().map(|o| ObservationId::new(*o)).collect(),
            contributing_sources: src.iter().map(|s| SourceId::new(*s)).collect(),
            confidence_before: before,
            confidence_after: after,
            notes: String::new(),
            prev_provenance_id: prev.map(ProvenanceId::new),
        }
    }

    #[test]
    fn append_only_is_enforced_and_queries_answer() {
        let store = InMemoryProvenanceStore::new();
        let track = TrackId::new("trk-1");

        store.append(rec("p1", 1, ProvenanceOp::Created, None, 0.6, None, &["o1"], &["s1"])).unwrap();
        store.append(rec("p2", 2, ProvenanceOp::Merged, Some(0.6), 0.7, Some("p1"), &["o2"], &["s2"])).unwrap();
        store.append(rec("p3", 3, ProvenanceOp::ConflictPreserved, Some(0.7), 0.5, Some("p2"), &["o3"], &["s3"])).unwrap();

        // append-only: a duplicate id and a dangling prev link are both rejected.
        assert!(store.append(rec("p1", 9, ProvenanceOp::Updated, None, 0.1, None, &[], &[])).is_err());
        assert!(store.append(rec("p9", 9, ProvenanceOp::Updated, Some(0.5), 0.5, Some("nope"), &[], &[])).is_err());

        // "why does this track exist?" -> the Created record.
        assert_eq!(store.why_does_track_exist(&track).unwrap().provenance_id, ProvenanceId::new("p1"));

        // "which observations contributed?" -> de-duplicated union, in order.
        assert_eq!(
            store.contributing_observations(&track).unwrap(),
            vec![ObservationId::new("o1"), ObservationId::new("o2"), ObservationId::new("o3")]
        );

        // "which source lowered confidence?" -> s3 (0.7 -> 0.5).
        let lowered = store.sources_that_lowered_confidence(&track).unwrap();
        assert_eq!(lowered.len(), 1);
        assert_eq!(lowered[0].sources, vec![SourceId::new("s3")]);
        assert!(lowered[0].confidence_delta < 0.0);

        // "what changed since the previous version?" -> v2 -> v3.
        let diff = store.changed_since_previous(&track).unwrap().unwrap();
        assert_eq!((diff.from_version, diff.to_version), (2, 3));
        assert_eq!(diff.added_sources, vec![SourceId::new("s3")]);
    }
}
