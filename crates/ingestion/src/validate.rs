//! Schema validation for inbound observations.
//!
//! Runs before anything reaches the fusion engine. Cheap structural and range
//! checks plus the signed-event check; rejects rather than fusing garbage.

use shared_types::{FabricError, Observation, Result, SCHEMA_VERSION};

pub fn validate(obs: &Observation) -> Result<()> {
    if obs.schema_version != SCHEMA_VERSION {
        return Err(FabricError::Validation(format!(
            "unsupported schema_version {} (expected {SCHEMA_VERSION})",
            obs.schema_version
        )));
    }
    if obs.observation_id.as_str().is_empty() {
        return Err(FabricError::Validation("empty observation_id".into()));
    }
    if obs.source_id.as_str().is_empty() {
        return Err(FabricError::Validation("empty source_id".into()));
    }
    if !(0.0..=1.0).contains(&obs.measurement_confidence) {
        return Err(FabricError::Validation(format!(
            "measurement_confidence {} out of [0,1]",
            obs.measurement_confidence
        )));
    }
    if obs.observed_at.millis() < 0 || obs.received_at.millis() < 0 {
        return Err(FabricError::Validation("negative timestamp".into()));
    }
    if let Some(state) = obs.state {
        if !state.position.iter().all(|v| v.is_finite())
            || !state.velocity.iter().all(|v| v.is_finite())
        {
            return Err(FabricError::Validation("non-finite state value".into()));
        }
        if state.position_sigma_m < 0.0 || !state.position_sigma_m.is_finite() {
            return Err(FabricError::Validation("invalid position_sigma_m".into()));
        }
    }
    if !obs.signature.verify(&[]) {
        return Err(FabricError::SignatureInvalid(format!(
            "algorithm {}",
            obs.signature.algorithm
        )));
    }
    Ok(())
}
