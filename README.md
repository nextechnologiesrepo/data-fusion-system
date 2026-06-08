# Assured Edge Fusion and Confidence Fabric

A real-time **edge assurance/fusion layer** that ingests heterogeneous
ISR/EW/platform/sensor data (synthetic in this prototype), normalizes it, tracks
provenance, calibrates uncertainty, fuses observations into machine-readable
tracks/events, and exposes confidence to operators and downstream systems.

It is designed to sit **underneath** existing C2, autonomy, ISR, and edge-compute
stacks — **not** to be a battle-management platform. It does not decide; it makes
the inputs to decisions traceable, calibrated, and machine-readable.

> Prototype status: synthetic data only, single-node, local-first. See
> [docs/threat-model.md](docs/threat-model.md) for what is enforced vs. stubbed.

## Quickstart

```bash
# Build + test everything
cargo test

# Run the API (binds 127.0.0.1:8088 by default — local only)
cargo run -p api

# Run the bundled deterministic scenario and read the metrics
curl -s -XPOST localhost:8088/api/v1/replay/start \
  -H 'content-type: application/json' -d '{"scenario":"scenario-01"}'
curl -s localhost:8088/api/v1/metrics

# Full stack (API + dashboard) in containers
docker compose up      # dashboard at http://localhost:5173
```

## Repository layout

```
docs/        architecture · data-model · api · replay-and-evaluation · threat-model · openapi.yaml
crates/
  shared-types        canonical data model (the stable contract)
  ingestion           adapter trait, synthetic generators, validation, rate limiting   [replaceable edge]
  fusion-core         FusionPolicy trait + FusionEngine orchestration                   [core]
  confidence-engine   ConfidenceScorer trait + baseline scorer                          [core]
  provenance-store    append-only store + provenance queries                            [core]
  replay              deterministic harness + 8 evaluation metrics                      [core]
services/api          Axum HTTP surface + OpenAPI                                        [thin glue]
apps/dashboard        minimal read-only operator view                                   [replaceable edge]
sim/                  scenario specs + generator docs
tests/                fixtures + end-to-end integration tests
```

## The architecture in one paragraph

Adapters emit `Observation`s carrying a native payload plus a normalized
`StateEstimate`. The API validates and rate-limits them; the `FusionEngine`
rejects stale data, asks a pluggable `FusionPolicy` how each observation relates
to current tracks (start / merge / **preserve as conflict**), and publishes a
`FusedTrack`. A **separate** confidence engine scores each track —
score + uncertainty band + machine-readable reason codes + degradation reason +
calibration status. Every change writes an **append-only** `ProvenanceRecord`, so
the API can answer "why does this track exist?", "which observations contributed?",
"which source lowered confidence?", and "what changed since the last version?". The
replay harness reruns synthetic scenarios deterministically and scores them with
eight evaluation metrics. The proprietary value — fusion policy, confidence
scoring, provenance, evaluation — is isolated behind traits; ingestion, schemas,
and the UI are meant to be replaced.

## What's deliberately simple in v0

Deterministic nearest-neighbour association (not a real tracker), a static
calibration score, `none-v0` signed-event placeholder, in-memory + append-only
JSONL persistence (SQLite planned), no auth/TLS, read-only dashboard. Each lives
behind a trait so it can be replaced without touching the fusion contract.

## Docs

- [Architecture](docs/architecture.md)
- [Data model](docs/data-model.md)
- [API](docs/api.md) · [OpenAPI](docs/openapi.yaml)
- [Replay & evaluation](docs/replay-and-evaluation.md)
- [Threat model](docs/threat-model.md)
