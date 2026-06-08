# Replay and Evaluation

The harness in [`crates/replay`](../crates/replay) reruns synthetic event logs
through a **fresh** fusion engine under fixed seeds and scores the result. Because
nothing on the fusion path touches the OS clock or RNG, a scenario produces
identical tracks, provenance, and metrics on every run — the determinism test
asserts exactly this.

## Scenario format

A scenario (`sim/scenarios/<name>.json`) is a compact, declarative spec. The
harness materializes it into a concrete observation stream (one observation per
emitter per timeline step) and feeds it to the engine in timestamp order.

```jsonc
{
  "schema_version": 1,
  "name": "two-target-crossing",
  "seed": 1337,
  "deterministic": true,
  "ground_truth_tracks": 3,          // real targets, for false/missed rates
  "config": {                        // optional engine overrides
    "staleness_limit_ms": 5000,
    "freshness_horizon_ms": 10000,
    "default_calibration_score": 0.8
  },
  "timeline": { "start_ms": 10000, "end_ms": 16000, "step_ms": 1000 },
  "sources":  [ { "source_id": "radar-alpha", "kind": "radar", "reliability": 0.9 } ],
  "health":   [ { "source_id": "eoir-charlie", "status": "degraded", "health_score": 0.4, "detail": "…" } ],
  "emitters": [ { "source_id": "radar-alpha", "kind": "radar", "seed": 11,
                  "target": { "start": [1000,5000,200], "velocity": [-20,5,0] } } ],
  "feedback": [ { "at_ms": 14000, "track": "trk-000001", "operator": "op-1",
                  "verdict": "confirm_track", "note": "visual ID" } ]
}
```

- **emitter `kind`**: `radar` · `eoir` · `ew_sigint` · `platform` (maps to the
  matching synthetic generator).
- **`observed_lag_ms`** on an emitter backdates `observed_at` to exercise stale
  rejection.
- **`classification`** (EO/IR) and **`frequency_mhz`** (EW) are optional per kind.

The bundled `scenario-01.json` ("two-target-crossing") exercises every path:
two radars corroborating one target (confirm-by-sources), a degraded EO/IR sensor
whose offset detections are **preserved as conflicts**, an EW + radar pair on a
second target, ownship PNT (confirm-by-observation-count), a laggy radar whose
reports are **rejected as stale**, and an operator confirmation.

## The eight metrics

Computed as pure functions of a post-run snapshot (`metrics::compute`), so scoring
is itself deterministic and unit-testable.

| Metric (`MetricKind`) | Unit | Definition in v0 |
|---|---|---|
| `track_confirmation_latency_ms` | ms | mean of `confirmed_at − first_observation_at` over confirmed tracks |
| `false_track_rate` | ratio | `max(0, confirmed − ground_truth) / max(confirmed,1)` |
| `missed_track_rate` | ratio | `max(0, ground_truth − confirmed) / max(ground_truth,1)` |
| `confidence_calibration_error` | abs_error | `|mean_confidence(confirmed) − true_positive_fraction|` |
| `stale_data_rejection_count` | count | observations rejected by the staleness gate |
| `conflict_rate` | ratio | `total_preserved_conflicts / accepted_observations` |
| `operator_override_rate` | ratio | `operator_feedback_events / accepted_observations` |
| `provenance_completeness` | ratio | fraction of tracks whose chain starts with `Created` **and** covers every contributing observation |

### Reference output (`scenario-01`)

Running the bundled scenario yields (deterministically):

| metric | value |
|---|---|
| track_confirmation_latency_ms | 666.67 |
| false_track_rate | 0.000 |
| missed_track_rate | 0.000 |
| confidence_calibration_error | 0.140 |
| stale_data_rejection_count | 7 |
| conflict_rate | 0.167 (7 / 42) |
| operator_override_rate | 0.024 (1 / 42) |
| provenance_completeness | 1.000 |

## Running it

```bash
cargo test -p replay                                   # determinism + metric tests
cargo run -p api & curl -s -XPOST localhost:8088/api/v1/replay/start \
  -H 'content-type: application/json' -d '{"scenario":"scenario-01"}'   # via API
```

## Interpreting the metrics as a calibration loop

`confidence_calibration_error` is the signal the calibration loop will eventually
minimize: it compares the confidence the fabric *claimed* against how often
confirmed tracks were actually true. In v0 the calibration score is static
(`default_calibration_score`); a future loop adjusts it per source/scenario so the
error trends toward zero. Replay is the closed-loop test bed for that work.
