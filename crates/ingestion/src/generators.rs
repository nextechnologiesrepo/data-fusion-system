//! Synthetic mock generators — one per source family.
//!
//! All generators are deterministic given a seed and the scenario start time, so
//! a scenario built from them replays identically. Each produces the *native*
//! payload for its family plus the normalized [`StateEstimate`] the fusion core
//! gates on (except the operator console, which is non-kinematic).

use shared_types::{
    Observation, ObservationId, ObservationPayload, Signature, Source, SourceId, SourceKind,
    StateEstimate, Timestamp, TrackId, SCHEMA_VERSION,
};

use crate::adapter::SourceAdapter;
use crate::rng::DeterministicRng;

/// A constant-velocity target in the local ENU frame (metres, metres/second).
#[derive(Debug, Clone, Copy)]
pub struct SyntheticTarget {
    pub start: [f64; 3],
    pub velocity: [f64; 3],
}

impl SyntheticTarget {
    pub fn new(start: [f64; 3], velocity: [f64; 3]) -> Self {
        SyntheticTarget { start, velocity }
    }

    fn position_at(&self, t0: Timestamp, now: Timestamp) -> [f64; 3] {
        let dt = (now.millis() - t0.millis()) as f64 / 1000.0;
        [
            self.start[0] + self.velocity[0] * dt,
            self.start[1] + self.velocity[1] * dt,
            self.start[2] + self.velocity[2] * dt,
        ]
    }
}

fn range_bearing_elev(p: [f64; 3]) -> (f64, f64, f64) {
    let range = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-6);
    let bearing = p[0].atan2(p[1]).to_degrees().rem_euclid(360.0);
    let elevation = (p[2] / range).asin().to_degrees();
    (range, bearing, elevation)
}

fn jitter(p: [f64; 3], rng: &mut DeterministicRng, sigma: f64) -> [f64; 3] {
    [
        p[0] + rng.noise(sigma),
        p[1] + rng.noise(sigma),
        p[2] + rng.noise(sigma),
    ]
}

fn next_id(source: &SourceId, seq: u64) -> ObservationId {
    ObservationId::new(format!("{}-{:06}", source.as_str(), seq))
}

// ---------------------------------------------------------------------------
// Radar
// ---------------------------------------------------------------------------

pub struct RadarGenerator {
    source: Source,
    target: SyntheticTarget,
    t0: Timestamp,
    sigma: f64,
    rng: DeterministicRng,
    seq: u64,
}

impl RadarGenerator {
    pub fn new(source_id: &str, target: SyntheticTarget, t0: Timestamp, seed: u64) -> Self {
        RadarGenerator {
            source: Source::new(SourceId::new(source_id), SourceKind::Radar, source_id, 0.9, t0),
            target,
            t0,
            sigma: 25.0,
            rng: DeterministicRng::new(seed),
            seq: 0,
        }
    }

    pub fn emit(&mut self, now: Timestamp) -> Observation {
        self.seq += 1;
        let truth = self.target.position_at(self.t0, now);
        let pos = jitter(truth, &mut self.rng, self.sigma);
        let (range_m, bearing_deg, elevation_deg) = range_bearing_elev(pos);
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: next_id(&self.source.source_id, self.seq),
            source_id: self.source.source_id.clone(),
            source_kind: SourceKind::Radar,
            observed_at: now,
            received_at: now,
            payload: ObservationPayload::RadarTrack {
                range_m,
                bearing_deg,
                elevation_deg,
                radial_velocity_mps: 0.0,
            },
            state: Some(StateEstimate {
                position: pos,
                velocity: self.target.velocity,
                position_sigma_m: self.sigma,
            }),
            measurement_confidence: 0.85,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }
}

impl SourceAdapter for RadarGenerator {
    fn source(&self) -> &Source {
        &self.source
    }
    fn poll(&mut self, now: Timestamp) -> Vec<Observation> {
        vec![self.emit(now)]
    }
}

// ---------------------------------------------------------------------------
// EO/IR
// ---------------------------------------------------------------------------

pub struct EoIrGenerator {
    source: Source,
    target: SyntheticTarget,
    t0: Timestamp,
    sigma: f64,
    classification: String,
    rng: DeterministicRng,
    seq: u64,
}

impl EoIrGenerator {
    pub fn new(
        source_id: &str,
        target: SyntheticTarget,
        classification: &str,
        t0: Timestamp,
        seed: u64,
    ) -> Self {
        EoIrGenerator {
            source: Source::new(SourceId::new(source_id), SourceKind::EoIr, source_id, 0.75, t0),
            target,
            t0,
            sigma: 60.0,
            classification: classification.to_string(),
            rng: DeterministicRng::new(seed),
            seq: 0,
        }
    }

    pub fn emit(&mut self, now: Timestamp) -> Observation {
        self.seq += 1;
        let truth = self.target.position_at(self.t0, now);
        let pos = jitter(truth, &mut self.rng, self.sigma);
        let (_r, bearing_deg, elevation_deg) = range_bearing_elev(pos);
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: next_id(&self.source.source_id, self.seq),
            source_id: self.source.source_id.clone(),
            source_kind: SourceKind::EoIr,
            observed_at: now,
            received_at: now,
            payload: ObservationPayload::EoIrDetection {
                bearing_deg,
                elevation_deg,
                classification: self.classification.clone(),
                pixel_intensity: 0.5 + self.rng.unit() * 0.5,
            },
            state: Some(StateEstimate {
                position: pos,
                velocity: self.target.velocity,
                position_sigma_m: self.sigma,
            }),
            measurement_confidence: 0.7,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }
}

impl SourceAdapter for EoIrGenerator {
    fn source(&self) -> &Source {
        &self.source
    }
    fn poll(&mut self, now: Timestamp) -> Vec<Observation> {
        vec![self.emit(now)]
    }
}

// ---------------------------------------------------------------------------
// EW / SIGINT
// ---------------------------------------------------------------------------

pub struct EwEmitterGenerator {
    source: Source,
    target: SyntheticTarget,
    t0: Timestamp,
    sigma: f64,
    frequency_mhz: f64,
    rng: DeterministicRng,
    seq: u64,
}

impl EwEmitterGenerator {
    pub fn new(
        source_id: &str,
        target: SyntheticTarget,
        frequency_mhz: f64,
        t0: Timestamp,
        seed: u64,
    ) -> Self {
        EwEmitterGenerator {
            source: Source::new(
                SourceId::new(source_id),
                SourceKind::EwSigint,
                source_id,
                0.6,
                t0,
            ),
            target,
            t0,
            sigma: 200.0,
            frequency_mhz,
            rng: DeterministicRng::new(seed),
            seq: 0,
        }
    }

    pub fn emit(&mut self, now: Timestamp) -> Observation {
        self.seq += 1;
        let truth = self.target.position_at(self.t0, now);
        let pos = jitter(truth, &mut self.rng, self.sigma);
        let (_r, bearing_deg, _e) = range_bearing_elev(pos);
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: next_id(&self.source.source_id, self.seq),
            source_id: self.source.source_id.clone(),
            source_kind: SourceKind::EwSigint,
            observed_at: now,
            received_at: now,
            payload: ObservationPayload::EwEmitter {
                bearing_deg,
                frequency_mhz: self.frequency_mhz,
                modulation: "pulsed".to_string(),
                signal_strength_dbm: -60.0 + self.rng.noise(5.0),
            },
            // Coarse positional estimate (bearing-only fusion is a v0 simplification).
            state: Some(StateEstimate {
                position: pos,
                velocity: [0.0, 0.0, 0.0],
                position_sigma_m: self.sigma,
            }),
            measurement_confidence: 0.6,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }
}

impl SourceAdapter for EwEmitterGenerator {
    fn source(&self) -> &Source {
        &self.source
    }
    fn poll(&mut self, now: Timestamp) -> Vec<Observation> {
        vec![self.emit(now)]
    }
}

// ---------------------------------------------------------------------------
// Platform state / PNT
// ---------------------------------------------------------------------------

pub struct PlatformStateGenerator {
    source: Source,
    target: SyntheticTarget,
    t0: Timestamp,
    rng: DeterministicRng,
    seq: u64,
}

impl PlatformStateGenerator {
    pub fn new(source_id: &str, target: SyntheticTarget, t0: Timestamp, seed: u64) -> Self {
        PlatformStateGenerator {
            source: Source::new(
                SourceId::new(source_id),
                SourceKind::Platform,
                source_id,
                0.95,
                t0,
            ),
            target,
            t0,
            rng: DeterministicRng::new(seed),
            seq: 0,
        }
    }

    pub fn emit(&mut self, now: Timestamp) -> Observation {
        self.seq += 1;
        let pos = self.target.position_at(self.t0, now);
        let heading = self.target.velocity[0].atan2(self.target.velocity[1]).to_degrees();
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: next_id(&self.source.source_id, self.seq),
            source_id: self.source.source_id.clone(),
            source_kind: SourceKind::Platform,
            observed_at: now,
            received_at: now,
            payload: ObservationPayload::PlatformState {
                // Toy local-frame → lat/lon mapping; synthetic only.
                lat_deg: pos[1] / 111_320.0,
                lon_deg: pos[0] / 111_320.0,
                alt_m: pos[2],
                heading_deg: heading.rem_euclid(360.0),
                pnt_quality: 0.9 + self.rng.unit() * 0.1,
            },
            state: Some(StateEstimate {
                position: pos,
                velocity: self.target.velocity,
                position_sigma_m: 5.0,
            }),
            measurement_confidence: 0.95,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }
}

impl SourceAdapter for PlatformStateGenerator {
    fn source(&self) -> &Source {
        &self.source
    }
    fn poll(&mut self, now: Timestamp) -> Vec<Observation> {
        vec![self.emit(now)]
    }
}

// ---------------------------------------------------------------------------
// Operator console (non-kinematic override events)
// ---------------------------------------------------------------------------

pub struct OperatorOverrideGenerator {
    source: Source,
    seq: u64,
}

impl OperatorOverrideGenerator {
    pub fn new(source_id: &str, t0: Timestamp) -> Self {
        OperatorOverrideGenerator {
            source: Source::new(
                SourceId::new(source_id),
                SourceKind::Operator,
                source_id,
                1.0,
                t0,
            ),
            seq: 0,
        }
    }

    pub fn emit(&mut self, now: Timestamp, target_track: Option<TrackId>, directive: &str) -> Observation {
        self.seq += 1;
        Observation {
            schema_version: SCHEMA_VERSION,
            observation_id: next_id(&self.source.source_id, self.seq),
            source_id: self.source.source_id.clone(),
            source_kind: SourceKind::Operator,
            observed_at: now,
            received_at: now,
            payload: ObservationPayload::OperatorOverride {
                target_track,
                directive: directive.to_string(),
            },
            state: None,
            measurement_confidence: 1.0,
            provenance_ref: None,
            signature: Signature::unsigned(),
        }
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators_are_deterministic() {
        let target = SyntheticTarget::new([1000.0, 2000.0, 100.0], [10.0, 0.0, 0.0]);
        let mut a = RadarGenerator::new("radar-a", target, Timestamp(0), 7);
        let mut b = RadarGenerator::new("radar-a", target, Timestamp(0), 7);
        assert_eq!(a.emit(Timestamp(1000)), b.emit(Timestamp(1000)));
    }

    #[test]
    fn radar_observation_passes_validation() {
        let target = SyntheticTarget::new([1000.0, 2000.0, 100.0], [10.0, 0.0, 0.0]);
        let mut g = RadarGenerator::new("radar-a", target, Timestamp(0), 1);
        let obs = g.emit(Timestamp(1000));
        crate::validate::validate(&obs).unwrap();
    }
}
