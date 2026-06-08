//! File-backed append-only provenance store (JSON Lines).
//!
//! This is the v0 *local persistence* backend. It satisfies the edge-first
//! requirement that the node keep enough local state to keep operating while
//! disconnected, and to resynchronize later without corrupting history: the
//! file is only ever appended to, and on open it is fully replayed back into an
//! in-memory index. SQLite is the planned production backend (see threat-model
//! and architecture docs) and will implement the same [`ProvenanceStore`] trait.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use shared_types::{FabricError, ProvenanceId, ProvenanceRecord, Result, TrackId};

use crate::memory::InMemoryProvenanceStore;
use crate::ProvenanceStore;

pub struct JsonlProvenanceStore {
    path: PathBuf,
    index: InMemoryProvenanceStore,
    file: Mutex<File>,
}

impl JsonlProvenanceStore {
    /// Open (creating if absent) an append-only log at `path`, replaying any
    /// existing records into the in-memory index.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let index = InMemoryProvenanceStore::new();

        if path.exists() {
            let file = File::open(&path).map_err(|e| FabricError::Io(e.to_string()))?;
            for (lineno, line) in BufReader::new(file).lines().enumerate() {
                let line = line.map_err(|e| FabricError::Io(e.to_string()))?;
                if line.trim().is_empty() {
                    continue;
                }
                let record: ProvenanceRecord = serde_json::from_str(&line).map_err(|e| {
                    FabricError::ProvenanceBroken(format!("corrupt record at line {}: {e}", lineno + 1))
                })?;
                index.append(record)?;
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| FabricError::Io(e.to_string()))?;

        Ok(JsonlProvenanceStore {
            path,
            index,
            file: Mutex::new(file),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ProvenanceStore for JsonlProvenanceStore {
    fn append(&self, record: ProvenanceRecord) -> Result<()> {
        // Validate + index first so a rejected record never touches the file.
        self.index.append(record.clone())?;

        let line = serde_json::to_string(&record)?;
        let mut file = self.file.lock().expect("provenance file lock poisoned");
        writeln!(file, "{line}").map_err(|e| FabricError::Io(e.to_string()))?;
        file.flush().map_err(|e| FabricError::Io(e.to_string()))?;
        Ok(())
    }

    fn get(&self, id: &ProvenanceId) -> Result<Option<ProvenanceRecord>> {
        self.index.get(id)
    }

    fn chain_for_track(&self, track: &TrackId) -> Result<Vec<ProvenanceRecord>> {
        self.index.chain_for_track(track)
    }

    fn all(&self) -> Result<Vec<ProvenanceRecord>> {
        self.index.all()
    }
}
