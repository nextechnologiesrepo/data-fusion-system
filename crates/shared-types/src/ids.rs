//! Strongly-typed identifiers.
//!
//! Every ID is a newtype around `String` so the compiler stops you handing a
//! `SourceId` where a `TrackId` is expected. IDs are plain strings (not random
//! UUIDs) so that scenarios and replay logs can pin them and stay deterministic.

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                $name(s.into())
            }

            /// Deterministic ID from a sequence number, e.g. `obs-000007`.
            pub fn seq(n: u64) -> Self {
                $name(format!("{}-{:06}", $prefix, n))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }
    };
}

string_id!(/// Identifies a registered data source (one sensor/feed/operator console).
    SourceId, "src");
string_id!(/// Identifies a single observation emitted by a source.
    ObservationId, "obs");
string_id!(/// Stable identity of a fused track across its versions.
    TrackId, "trk");
string_id!(/// Identifies a single track hypothesis under consideration.
    HypothesisId, "hyp");
string_id!(/// Identifies one append-only provenance record.
    ProvenanceId, "prv");
string_id!(/// Identifies a recommendation cue surfaced to an operator.
    CueId, "cue");
string_id!(/// Identifies an operator feedback / override event.
    FeedbackId, "fbk");
string_id!(/// Identifies a replay session.
    SessionId, "ses");
