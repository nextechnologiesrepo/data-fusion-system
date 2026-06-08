//! Replay and evaluation harness.
//!
//! A [`Scenario`] is a compact, deterministic description of a synthetic event
//! log: which sources exist, how healthy they are, and which targets each
//! emitter observes over a timeline. [`ReplayHarness`] materializes that into a
//! concrete observation stream, runs it through a fresh [`fusion_core::FusionEngine`]
//! under fixed seeds, and scores the run with the eight [`metrics`].
//!
//! Determinism is the whole point: same scenario → identical tracks, provenance,
//! and metrics, every run, so regressions in fusion or confidence logic are
//! visible as metric deltas.

pub mod harness;
pub mod metrics;
pub mod scenario;

pub use harness::{ReplayHarness, ReplayReport};
pub use metrics::{compute as compute_metrics, MetricInputs};
pub use scenario::{
    EmitterKind, EmitterSpec, FeedbackSpec, HealthSpec, Scenario, SourceSpec, TargetSpec, Timeline,
};
