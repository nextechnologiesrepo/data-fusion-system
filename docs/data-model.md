# Canonical Data Model

All objects live in [`crates/shared-types`](../crates/shared-types) and are the
only types that cross crate boundaries on the data path. Shared rules:

- **`schema_version`** on every object (current = `1`, the `SCHEMA_VERSION` const).
  Bump on any breaking change; persisted/replayed records carry the version they
  were written with.
- **Logical time.** Timestamps are `i64` epoch-milliseconds (`Timestamp`), never
  wall-clock `DateTime`. The fusion path receives time from an injected `Clock`
  (`SystemClock` live, `ManualClock` in replay) so runs are deterministic.
- **Typed IDs.** `SourceId`, `ObservationId`, `TrackId`, `HypothesisId`,
  `ProvenanceId`, `CueId`, `FeedbackId`, `SessionId` are newtypes over `String`.
  They are plain strings (e.g. `trk-000001`), not random UUIDs, so scenarios and
  replay logs can pin them.
- **Confidence is never a bare float** on a published track — it is a
  `ConfidenceVector` (score + band + reasons + calibration).
- **Provenance reference.** Fused outputs carry a `provenance_ref` into the
  append-only chain.

## Shared building blocks

### `StateEstimate`
Normalized kinematics in a local **ENU** frame so fusion can gate generically.
| field | type | meaning |
|---|---|---|
| `position` | `[f64;3]` | East, North, Up (metres) |
| `velocity` | `[f64;3]` | m/s |
| `position_sigma_m` | `f64` | 1-σ positional uncertainty (m) |

### `Signature` (signed-event placeholder)
| field | type | meaning |
|---|---|---|
| `algorithm` | `String` | `none-v0` in this prototype |
| `key_id` | `String` | signing key id (`unsigned`) |
| `value` | `String` | hex signature bytes (empty for `none-v0`) |

`verify()` returns `true` only for `none-v0`; real algorithms fail closed until
implemented.

---

## 1. `Source`
A registered data feed (sensor, telemetry, or operator console).

| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `source_id` | `SourceId` | |
| `kind` | `SourceKind` | `radar` · `eo_ir` · `ew_sigint` · `platform` · `operator` |
| `name` | `String` | |
| `reliability` | `f64` | prior in `[0,1]`, used by confidence engine |
| `registered_at` | `Timestamp` | |

## 2. `Observation`
One report from a source. Carries native payload **and** normalized state.

| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `observation_id` | `ObservationId` | |
| `source_id` / `source_kind` | `SourceId` / `SourceKind` | |
| `observed_at` | `Timestamp` | drives staleness + freshness |
| `received_at` | `Timestamp` | ingest time = "now" for staleness |
| `payload` | `ObservationPayload` | tagged enum (below) |
| `state` | `Option<StateEstimate>` | `None` ⇒ non-kinematic (e.g. operator) |
| `measurement_confidence` | `f64` | source-reported, `[0,1]` |
| `provenance_ref` | `Option<ProvenanceId>` | |
| `signature` | `Signature` | |

**`ObservationPayload`** (serde tag = `kind`): `radar_track` {range, bearing,
elevation, radial_velocity} · `eo_ir_detection` {bearing, elevation,
classification, pixel_intensity} · `ew_emitter` {bearing, frequency_mhz,
modulation, signal_strength_dbm} · `platform_state` {lat, lon, alt, heading,
pnt_quality} · `operator_override` {target_track?, directive}.

## 3. `SensorHealth`
| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `source_id` | `SourceId` | |
| `reported_at` | `Timestamp` | |
| `status` | `HealthStatus` | `nominal`·`degraded`·`faulted`·`offline` |
| `health_score` | `f64` | `[0,1]`, fed into confidence |
| `detail` | `String` | |

## 4. `ProvenanceRecord` (append-only)
One immutable record per change to a fused track; records chain via
`prev_provenance_id`.

| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `provenance_id` | `ProvenanceId` | |
| `track_id` | `TrackId` | |
| `fused_version` | `u64` | version this record produced |
| `created_at` | `Timestamp` | |
| `operation` | `ProvenanceOp` | `created`·`merged`·`updated`·`conflict_preserved`·`operator_override` |
| `contributing_observations` | `Vec<ObservationId>` | obs added at this step |
| `contributing_sources` | `Vec<SourceId>` | source(s) responsible at this step |
| `confidence_before` / `confidence_after` | `Option<f64>` / `f64` | for attribution |
| `notes` | `String` | |
| `prev_provenance_id` | `Option<ProvenanceId>` | audit chain link |

## 5. `ConfidenceVector`
Output of the confidence engine for a fused track.

| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `computed_at` | `Timestamp` | |
| `score` | `f64` | aggregate `[0,1]` |
| `uncertainty_band` | `[f64;2]` | inclusive `[low, high]` |
| `reason_codes` | `Vec<ReasonCode>` | machine-readable (below) |
| `degradation_reason` | `Option<String>` | human text when degraded |
| `calibration_status` | `CalibrationStatus` | `calibrated`·`drifting`·`uncalibrated` |
| `components` | `ConfidenceComponents` | the 7 normalized inputs |

**`ReasonCode`**: `single_source_only`, `multi_source_corroboration`,
`stale_contribution`, `sensor_degraded`, `conflict_detected`, `operator_confirmed`,
`operator_rejected`, `low_source_reliability`, `well_calibrated`,
`calibration_drift`.

**`ConfidenceComponents`** (each `[0,1]`): `source_reliability`, `sensor_health`,
`freshness`, `corroboration`, `conflict` (1=none), `calibration_score`,
`operator_feedback` (0.5=neutral). These are exactly the seven required inputs and
are retained so an operator can see which lever moved the score.

## 6. `TrackHypothesis`
A candidate track under consideration (supporting vs conflicting observations,
source set, current state). Used internally by the fusion engine.

## 7. `FusedTrack`
The published, machine-readable track.

| field | type | notes |
|---|---|---|
| `schema_version` | `u32` | |
| `track_id` | `TrackId` | |
| `version` | `u64` | increments per change |
| `created_at` / `updated_at` | `Timestamp` | |
| `state` | `StateEstimate` | |
| `confidence` | `ConfidenceVector` | |
| `provenance_ref` | `ProvenanceId` | latest record |
| `contributing_observations` | `Vec<ObservationId>` | |
| `contributing_sources` | `Vec<SourceId>` | |
| `conflicts` | `Vec<ConflictRecord>` | preserved, never hidden |
| `status` | `TrackStatus` | `tentative`·`confirmed`·`coasting`·`dropped` |

`ConflictRecord` = {`observation_id`, `source_id`, `reason`, `divergence_m`}.

## 8. `RecommendationCue`
Advisory decision-support cue (the layer never decides).

| field | type | notes |
|---|---|---|
| `cue_id` | `CueId` | |
| `track_id` | `TrackId` | |
| `severity` | `CueSeverity` | `info`·`caution`·`warning` |
| `message` / `recommended_action` | `String` | |
| `confidence_ref` | `f64` | score snapshot at cue time |

## 9. `OperatorFeedback`
| field | type | notes |
|---|---|---|
| `feedback_id` | `FeedbackId` | |
| `track_id` | `TrackId` | |
| `operator_id` | `String` | |
| `submitted_at` | `Timestamp` | |
| `verdict` | `FeedbackVerdict` | `confirm_track`·`reject_track`·`reclassify`·`adjust_confidence` |
| `note` | `String` | |
| `confidence_adjustment` | `Option<f64>` | `[-1,1]` for `adjust_confidence` |

## 10. `ReplaySession`
| field | type | notes |
|---|---|---|
| `session_id` | `SessionId` | |
| `scenario_name` | `String` | |
| `started_at` / `finished_at` | `Timestamp` / `Option` | |
| `deterministic` | `bool` | |
| `seed` | `u64` | |
| `status` | `ReplayStatus` | `pending`·`running`·`completed`·`aborted` |
| `event_count` | `u64` | |

## 11. `EvaluationMetric`
| field | type | notes |
|---|---|---|
| `session_id` | `SessionId` | |
| `computed_at` | `Timestamp` | |
| `metric` | `MetricKind` | one of the eight (see [replay-and-evaluation.md](replay-and-evaluation.md)) |
| `value` | `f64` | |
| `unit` | `String` | `ms`·`ratio`·`count`·`abs_error` |
| `detail` | `String` | human explanation |
