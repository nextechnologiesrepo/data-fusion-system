//! Fusion policy: the pluggable association + merge decision.
//!
//! The policy answers two narrow questions and nothing else:
//!   1. how does an incoming observation relate to the current tracks?
//!   2. how do two kinematic states combine?
//!
//! Everything around it (staleness, provenance, confidence, cues) lives in the
//! [`crate::engine::FusionEngine`]. Keeping the policy this small is what lets
//! v0's deterministic gating be swapped for a real tracker later without
//! touching the assurance plumbing.

use shared_types::{Observation, StateEstimate, TrackId};

/// A minimal read-only view of a track, handed to the policy for association.
#[derive(Debug, Clone)]
pub struct TrackSnapshot {
    pub track_id: TrackId,
    pub state: StateEstimate,
}

/// The policy's verdict for one observation.
#[derive(Debug, Clone, PartialEq)]
pub enum Association {
    /// Start a new track from this observation.
    New,
    /// Merge into an existing track (consistent within the merge gate).
    Merge { track_id: TrackId, distance_m: f64 },
    /// Near an existing track but inconsistent — preserve as a conflict.
    Conflict { track_id: TrackId, divergence_m: f64 },
}

pub trait FusionPolicy: Send + Sync {
    /// Relate a (already de-staled, kinematic) observation to current tracks.
    fn associate(&self, obs: &Observation, tracks: &[TrackSnapshot]) -> Association;

    /// Combine an existing track state with an incoming observation state.
    fn merge_state(&self, current: &StateEstimate, incoming: &StateEstimate) -> StateEstimate;

    fn name(&self) -> &'static str;
}

/// Tunables for the baseline nearest-neighbour policy.
#[derive(Debug, Clone, Copy)]
pub struct GateConfig {
    /// Distance (m) within which an observation merges into a track.
    pub merge_gate_m: f64,
    /// Distance (m) within which a non-merging observation is kept as conflict.
    pub conflict_gate_m: f64,
    /// Extra gate margin added per metre of observation positional sigma.
    pub sigma_gate_factor: f64,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            merge_gate_m: 150.0,
            conflict_gate_m: 600.0,
            sigma_gate_factor: 1.0,
        }
    }
}

/// Deterministic nearest-neighbour gating. No randomness, no learned weights.
#[derive(Debug, Default, Clone, Copy)]
pub struct NearestNeighborPolicy {
    pub gate: GateConfig,
}

impl NearestNeighborPolicy {
    pub fn new(gate: GateConfig) -> Self {
        NearestNeighborPolicy { gate }
    }
}

impl FusionPolicy for NearestNeighborPolicy {
    fn associate(&self, obs: &Observation, tracks: &[TrackSnapshot]) -> Association {
        let Some(state) = obs.state else {
            // No normalized kinematics → cannot associate in v0; treat as new.
            return Association::New;
        };

        // Nearest track by Euclidean position distance. Ties broken by track_id
        // ordering so the choice is deterministic.
        let nearest = tracks
            .iter()
            .map(|t| (t, state.distance_to(&t.state)))
            .min_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.track_id.cmp(&b.0.track_id))
            });

        let Some((track, dist)) = nearest else {
            return Association::New;
        };

        let margin = self.gate.sigma_gate_factor * state.position_sigma_m;
        if dist <= self.gate.merge_gate_m + margin {
            Association::Merge {
                track_id: track.track_id.clone(),
                distance_m: dist,
            }
        } else if dist <= self.gate.conflict_gate_m + margin {
            Association::Conflict {
                track_id: track.track_id.clone(),
                divergence_m: dist,
            }
        } else {
            Association::New
        }
    }

    fn merge_state(&self, current: &StateEstimate, incoming: &StateEstimate) -> StateEstimate {
        // Inverse-variance weighted average — the explainable v0 stand-in for a
        // Kalman update. Tighter (smaller sigma) measurements pull harder.
        const EPS: f64 = 1e-6;
        let wc = 1.0 / (current.position_sigma_m.powi(2) + EPS);
        let wi = 1.0 / (incoming.position_sigma_m.powi(2) + EPS);
        let w = wc + wi;

        let blend = |a: [f64; 3], b: [f64; 3]| {
            [
                (a[0] * wc + b[0] * wi) / w,
                (a[1] * wc + b[1] * wi) / w,
                (a[2] * wc + b[2] * wi) / w,
            ]
        };

        StateEstimate {
            position: blend(current.position, incoming.position),
            velocity: blend(current.velocity, incoming.velocity),
            position_sigma_m: (1.0 / w).sqrt(),
        }
    }

    fn name(&self) -> &'static str {
        "nearest-neighbor-v0"
    }
}
