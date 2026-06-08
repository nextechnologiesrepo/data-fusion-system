//! The adapter contract.
//!
//! A [`SourceAdapter`] is the only thing the rest of the system knows about a
//! data source. Real connectors (DDS, MAVLink, a SIGINT bus, an operator
//! console) and the synthetic mock generators implement the same trait, so the
//! fusion path never changes when a source is swapped.

use shared_types::{Observation, Source, Timestamp};

pub trait SourceAdapter: Send {
    /// Static description of this source (id, kind, reliability prior).
    fn source(&self) -> &Source;

    /// Produce any observations available as of `now`. Synthetic generators
    /// return one scripted observation per call; a real adapter would drain its
    /// inbound queue. Returning an empty vec is normal.
    fn poll(&mut self, now: Timestamp) -> Vec<Observation>;
}
