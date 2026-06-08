# Architecture — Assured Edge Fusion and Confidence Fabric

> **What this is.** A narrow, composable *assurance and fusion layer* that sits
> **underneath** existing C2, autonomy, ISR, and edge-compute stacks. It ingests
> heterogeneous (synthetic, in this prototype) sensor data, normalizes it, tracks
> provenance, calibrates uncertainty, fuses observations into machine-readable
> tracks, and exposes confidence to humans and downstream systems.
>
> **What this is not.** It is *not* a battle-management platform, a C2 product, or
> a sensor. It does not make decisions. It makes the inputs to decisions
> trustworthy, explainable, and machine-readable.

## 1. Design goals

| Goal | How the architecture meets it |
|------|-------------------------------|
| **Edge-first** | Single Rust binary, SQLite/JSONL local state, `127.0.0.1` default bind, internal async bus — no cluster, broker, or cloud required to run. |
| **Degraded/disconnected operation** | All state is local and append-only; the node keeps fusing offline and can resynchronize later without rewriting history. |
| **Trust in automation** | A dedicated confidence engine emits a score, an uncertainty band, machine-readable reason codes, and a degradation reason for every output. |
| **Provenance** | Every fused output links back to its contributing observations and sources through an append-only provenance chain. |
| **Replaceable edges, proprietary core** | Adapters, schemas, and fixtures are clean and swappable. The value — fusion policy, confidence scoring, calibration, evaluation — is isolated behind traits. |

## 2. Components

```
                 ┌─────────────────────────────────────────────────────────────┐
                 │                        services/api (Axum)                   │
                 │  /observations  /tracks  /…/provenance  /…/confidence        │
                 │  /feedback  /replay  /metrics  /health  /openapi.yaml        │
                 └───────▲───────────────▲───────────────▲────────────▲─────────┘
                         │               │               │            │
   ┌─────────────────────┴───┐   ┌───────┴────────┐  ┌───┴─────────┐  │
   │   ingestion (edge)      │   │  fusion-core   │  │ provenance- │  │
   │  ┌───────────────────┐  │   │  ┌──────────┐  │  │   store     │  │
   │  │ SourceAdapter     │  │   │  │ Fusion   │  │  │ append-only │  │
   │  │  · radar          │  │   │  │ Policy   │  │  │ (memory /   │  │
   │  │  · EO/IR          │──┼──▶│  │ (assoc/  │──┼─▶│  JSONL /    │  │
   │  │  · EW/SIGINT      │  │   │  │  merge)  │  │  │  SQLite*)   │  │
   │  │  · platform/PNT   │  │   │  └────┬─────┘  │  └─────────────┘  │
   │  │  · operator       │  │   │       │        │                   │
   │  ├───────────────────┤  │   │  ┌────▼──────────────┐             │
   │  │ validate + rate   │  │   │  │ FusionEngine      │             │
   │  │ limit + generators│  │   │  │ · staleness       │             │
   │  └───────────────────┘  │   │  │ · conflict keep   │             │
   └─────────────────────────┘   │  │ · operator fb     │             │
                                 │  └────┬──────────────┘             │
                                 │       │ calls                      │
                                 │  ┌────▼─────────────┐              │
                                 │  │ confidence-engine│              │
                                 │  │ (separate scorer)│              │
                                 │  └──────────────────┘              │
                                 └────────────────────────────────────┘
                  replay ───────────── drives the same path under a fixed clock ⇒ EvaluationMetric
   (* SQLite backend is planned; v0 ships in-memory + append-only JSONL.)
```

| Crate / dir | Role | Proprietary? |
|---|---|---|
| `crates/shared-types` | Canonical data model, IDs, logical time, clock trait | contract |
| `crates/ingestion` | Adapter trait, synthetic generators, validation, rate limiting | replaceable edge |
| `crates/fusion-core` | `FusionPolicy` trait + `FusionEngine` orchestration | **core** |
| `crates/confidence-engine` | `ConfidenceScorer` trait + baseline scorer | **core** |
| `crates/provenance-store` | Append-only store + the four provenance queries | **core** |
| `crates/replay` | Deterministic harness + evaluation metrics | **core** |
| `services/api` | Axum HTTP surface + OpenAPI | thin glue |
| `apps/dashboard` | Minimal read-only operator view | replaceable edge |
| `sim/` | Scenario specs + generator docs | fixtures |

## 3. Data flow

1. **Ingest.** An adapter (synthetic generator in v0) emits an `Observation` carrying
   its native payload *and* a normalized `StateEstimate`. The API validates it and
   charges a per-source token bucket.
2. **Normalize & gate.** `FusionEngine` treats the observation's `received_at` as
   "now", rejects it if `observed_at` is older than the staleness limit, and asks
   the `FusionPolicy` how it relates to current tracks.
3. **Associate / merge / preserve.** The policy returns `New`, `Merge`, or
   `Conflict`. New observations start a track; consistent ones merge (inverse-variance
   blend); inconsistent-but-nearby ones are **kept as conflicts, never discarded**.
4. **Score.** The engine assembles a flat `ConfidenceInputs` from the track's
   membership and the source/health registry, and the **separate** confidence engine
   returns a `ConfidenceVector` (score, band, reason codes, degradation reason,
   calibration status).
5. **Record.** The engine appends a `ProvenanceRecord` (`Created`/`Merged`/
   `ConflictPreserved`/`OperatorOverride`) linked to the previous one, then publishes
   the updated `FusedTrack` and an optional `RecommendationCue`.
6. **Expose.** The API serves tracks, provenance answers, confidence explanations,
   and evaluation metrics; operator feedback flows back in and re-scores the track.

## 4. Edge & resilience model

- **Single machine first.** `cargo run -p api` is the whole system. Docker Compose
  adds only the dashboard.
- **Local state.** Provenance is append-only; the JSONL backend rebuilds its index
  by replaying the file on open, so a crash or disconnection loses nothing and
  resync is just "append more lines". No in-place mutation ever corrupts history.
- **Determinism.** Nothing on the fusion path reads the OS clock or RNG directly —
  a `Clock` is injected and generators are seeded. This is what makes replay
  bit-for-bit reproducible and is the foundation of the evaluation harness.
- **Later deployment.** The same binary targets rugged hardware or an edge
  Kubernetes distro; an external bus (NATS/Redis Streams) can replace the in-process
  channel without touching the core, because ingestion is already trait-bounded.

## 5. Why the core is separable

The confidence engine, fusion policy, calibration loop, and evaluation harness are
each behind a trait (`ConfidenceScorer`, `FusionPolicy`, `ProvenanceStore`,
`ReplayHarness`). v0 ships deliberately simple, explainable implementations
(`nearest-neighbor-v0`, `baseline-v0`). Replacing any one of them — e.g. a learned
associator or a Platt-scaled calibrator — is a new `impl`, not a rewrite. Adapters
and schemas on the outside stay stable.

## 6. What is stubbed in v0

See [threat-model.md](threat-model.md) for the security posture. Functionally:
signed events are a placeholder (`none-v0`, verify-always), SQLite persistence is
deferred behind the in-memory/JSONL store, association is naive Euclidean gating,
calibration is a static score, auth/mTLS are absent, and the dashboard is read-only.
