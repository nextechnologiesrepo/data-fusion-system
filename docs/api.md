# API

The Axum service in [`services/api`](../services/api) exposes the fabric. The
machine-readable contract is [`openapi.yaml`](openapi.yaml), served live at
`GET /openapi.yaml`. Base URL defaults to `http://127.0.0.1:8088` (local-only).

Run it:

```bash
cargo run -p api                       # binds 127.0.0.1:8088 by default
FUSION_BIND=127.0.0.1:9000 cargo run -p api   # override bind (opt-in)
```

Environment:
- `FUSION_BIND` — listen address (default `127.0.0.1:8088`).
- `FUSION_SCENARIOS_DIR` — scenario directory (default `sim/scenarios`).
- `FUSION_OPENAPI` — path to the served spec (default `docs/openapi.yaml`).
- `RUST_LOG` — tracing filter (default `info`); logs are structured JSON.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/health`, `/healthz` | Liveness + version + uptime |
| GET | `/readyz` | Readiness + live track count |
| GET | `/api/v1/sources` | List registered sources |
| POST | `/api/v1/observations` | Submit an observation (validate → rate-limit → fuse) |
| GET | `/api/v1/tracks` | List fused tracks |
| GET | `/api/v1/tracks/{id}` | Get one fused track |
| GET | `/api/v1/tracks/{id}/provenance` | Provenance answers for a track |
| GET | `/api/v1/tracks/{id}/confidence` | Confidence explanation for a track |
| POST | `/api/v1/feedback` | Submit operator feedback / override |
| POST | `/api/v1/replay/start` | Run a scenario through a fresh engine |
| POST | `/api/v1/replay/stop` | Report the last replay result (v0 runs sync) |
| GET | `/api/v1/metrics` | Evaluation metrics from the last replay |
| GET | `/openapi.yaml` | This service's OpenAPI document |

## Examples

### Submit an observation
```bash
curl -s -X POST localhost:8088/api/v1/observations -H 'content-type: application/json' -d '{
  "schema_version":1,"observation_id":"obs-live-1","source_id":"radar-live",
  "source_kind":"radar","observed_at":1000,"received_at":1000,
  "payload":{"kind":"radar_track","range_m":100,"bearing_deg":10,"elevation_deg":2,"radial_velocity_mps":0},
  "state":{"position":[100,200,10],"velocity":[1,0,0],"position_sigma_m":20},
  "measurement_confidence":0.8,"provenance_ref":null,
  "signature":{"algorithm":"none-v0","key_id":"unsigned","value":""}}'
```
```json
{"accepted":true,"rejection":null,"operation":"Created",
 "track_id":"trk-000001","provenance_id":"prv-000001","cue":null,"note":null}
```
A second nearby observation from a *different* source returns `"operation":"Merged"`
and a `cue` of `"Track confirmed"`.

Rejections come back with `accepted:false` and a `rejection` string, e.g.
`"Stale { age_ms: 8000, limit_ms: 5000 }"`.

### Why does a track exist? (provenance)
```bash
curl -s localhost:8088/api/v1/tracks/trk-000001/provenance
```
Answers the four required questions in one payload:
- `why_exists` — the originating `Created` record.
- `contributing_observations` — de-duplicated across the chain.
- `sources_that_lowered_confidence` — each step whose confidence fell, attributed
  to the source(s) responsible.
- `changed_since_previous` — diff of the two newest fused versions.
- `chain` — the full append-only record list.

### Confidence explanation
```bash
curl -s localhost:8088/api/v1/tracks/trk-000001/confidence
```
Returns the full `ConfidenceVector` (score, uncertainty band, `reason_codes`,
`degradation_reason`, `calibration_status`, and the seven components) plus the
provenance attribution of any confidence drops.

### Operator feedback
```bash
curl -s -X POST localhost:8088/api/v1/feedback -H 'content-type: application/json' -d '{
  "track_id":"trk-000001","operator_id":"op-1","verdict":"confirm_track","note":"visual ID"}'
```
`reject_track` drops the track; `adjust_confidence` accepts a
`confidence_adjustment` in `[-1,1]`. Feedback is recorded as an append-only
`operator_override` provenance entry and re-scores the track.

### Replay + metrics
```bash
curl -s -X POST localhost:8088/api/v1/replay/start -H 'content-type: application/json' -d '{"scenario":"scenario-01"}'
curl -s localhost:8088/api/v1/metrics
```
Replay runs the named `sim/scenarios/<name>.json` through an isolated engine and
returns the session plus all eight evaluation metrics. See
[replay-and-evaluation.md](replay-and-evaluation.md).

## Errors
JSON body `{"error": "..."}` with status mapped from the core error:
`400` validation · `401` bad signature · `404` not found · `422` stale ·
`429` rate limited · `500` internal.
