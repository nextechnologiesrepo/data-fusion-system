//! In-memory append-only provenance store.
//!
//! The default backend for the prototype and for deterministic replay. Holds
//! records in insertion order plus an index by id; enforces the append-only and
//! chain-integrity invariants.

use std::collections::HashMap;
use std::sync::RwLock;

use shared_types::{FabricError, ProvenanceId, ProvenanceRecord, Result, TrackId};

use crate::ProvenanceStore;

#[derive(Default)]
pub struct InMemoryProvenanceStore {
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Insertion-ordered log — the source of truth.
    log: Vec<ProvenanceRecord>,
    /// provenance_id -> index into `log`.
    by_id: HashMap<ProvenanceId, usize>,
}

impl InMemoryProvenanceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.inner.read().expect("provenance lock poisoned").log.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ProvenanceStore for InMemoryProvenanceStore {
    fn append(&self, record: ProvenanceRecord) -> Result<()> {
        let mut inner = self.inner.write().expect("provenance lock poisoned");

        if inner.by_id.contains_key(&record.provenance_id) {
            return Err(FabricError::ProvenanceBroken(format!(
                "duplicate provenance_id {} (records are append-only)",
                record.provenance_id
            )));
        }
        if let Some(prev) = &record.prev_provenance_id {
            if !inner.by_id.contains_key(prev) {
                return Err(FabricError::ProvenanceBroken(format!(
                    "prev_provenance_id {prev} not found for {}",
                    record.provenance_id
                )));
            }
        }

        let idx = inner.log.len();
        inner.by_id.insert(record.provenance_id.clone(), idx);
        inner.log.push(record);
        Ok(())
    }

    fn get(&self, id: &ProvenanceId) -> Result<Option<ProvenanceRecord>> {
        let inner = self.inner.read().expect("provenance lock poisoned");
        Ok(inner.by_id.get(id).map(|&i| inner.log[i].clone()))
    }

    fn chain_for_track(&self, track: &TrackId) -> Result<Vec<ProvenanceRecord>> {
        let inner = self.inner.read().expect("provenance lock poisoned");
        let mut chain: Vec<ProvenanceRecord> = inner
            .log
            .iter()
            .filter(|r| &r.track_id == track)
            .cloned()
            .collect();
        chain.sort_by_key(|r| r.fused_version);
        Ok(chain)
    }

    fn all(&self) -> Result<Vec<ProvenanceRecord>> {
        let inner = self.inner.read().expect("provenance lock poisoned");
        Ok(inner.log.clone())
    }
}
