//! Canonical data model for the Assured Edge Fusion and Confidence Fabric.
//!
//! This crate is dependency-free apart from `serde`. Every other crate in the
//! workspace depends on these types, and nothing here depends on the engines —
//! the model is the stable contract at the centre of the system.
//!
//! Design rules honoured by every object in [`model`]:
//!   * carries a `schema_version` (see [`SCHEMA_VERSION`])
//!   * carries timestamps (logical epoch-millis, see [`time`])
//!   * references its source(s) and a provenance record where applicable
//!   * confidence is expressed through [`model::ConfidenceVector`], never a bare float

pub mod clock;
pub mod error;
pub mod ids;
pub mod model;
pub mod time;

pub use clock::{Clock, ManualClock, SystemClock};
pub use error::{FabricError, Result};
pub use ids::*;
pub use model::*;
pub use time::{Millis, Timestamp};

/// Current schema version stamped onto every canonical object.
///
/// Bump this when a breaking change is made to any struct in [`model`]. Replay
/// logs and persisted records record the version they were written with so a
/// reader can refuse or migrate older payloads.
pub const SCHEMA_VERSION: u32 = 1;
