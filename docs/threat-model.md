# Threat Model & Security/Reliability Baseline

Scope: a prototype on **synthetic data only**, single-node, local-first. This
document states the v0 posture honestly — what is enforced now, and what is a
deliberate placeholder — so nothing is mistaken for production assurance.

## Assets
- **Integrity of fused outputs** — a track and its confidence must be traceable and
  not silently corruptible.
- **Provenance history** — the append-only record is the audit source of truth.
- **Availability at the edge** — the node must keep working disconnected and
  resynchronize without rewriting history.

## Trust boundaries
```
[ source feeds ] → (1) ingestion boundary → [ fusion core ] → (2) API boundary → [ operator / downstream ]
                                                   │
                                                   └→ [ append-only provenance + local state ]
```
1. **Ingestion boundary** — untrusted, possibly spoofed or flooding sources.
2. **API boundary** — local clients in v0; the surface a future deployment must
   authenticate.

## Baseline controls (requirement 10)

| Control | Status in v0 | Where |
|---|---|---|
| **Signed event format** | *Placeholder.* `Signature{algorithm,key_id,value}` on every observation; `none-v0` verifies, any real algorithm fails closed until implemented. | `shared-types::model::Signature` |
| **Schema validation** | **Enforced.** Version, ID, range, finiteness, and signature checks before fusion. | `ingestion::validate` |
| **Input rate limiting** | **Enforced.** Per-source token bucket; excess → `429`. | `ingestion::RateLimiter`, API |
| **Structured audit logs** | **Enabled.** JSON tracing per request; provenance is the durable audit trail. | `services/api`, `provenance-store` |
| **Deterministic replay mode** | **Enforced.** Injected `Clock` + seeded generators ⇒ reproducible runs. | `shared-types::Clock`, `replay` |
| **Health endpoints** | **Enabled.** `/health`, `/healthz`, `/readyz`. | `services/api` |
| **Local-only default config** | **Enforced.** Binds `127.0.0.1`; non-local bind is explicit opt-in via `FUSION_BIND`. | `services/api::main` |
| **No hardcoded secrets** | **Enforced.** No credentials in source; config via env. `key_id` is a non-secret label. | repo-wide |
| **Component separation** | **Enforced.** Ingestion / fusion / confidence / provenance / API / UI are distinct crates behind traits. | workspace layout |

## Threats & v0 mitigations

| Threat | Mitigation now | Residual / planned |
|---|---|---|
| Spoofed/forged observation | Schema validation; signature field present | Real signature verification (Ed25519 over canonical bytes) |
| Source flooding / DoS | Per-source token bucket | Global backpressure, quota per kind |
| Stale/replayed data fused as current | Staleness gate rejects old `observed_at`; counted as a metric | Nonce/monotonic-sequence checks |
| Silent history rewrite | Append-only store rejects duplicate IDs and dangling `prev` links | Hash-chained / signed records, WORM storage |
| Confidence laundering (hiding disagreement) | Conflicts are **preserved** on the track and lower the score with a reason code | — |
| Unauthorized API access | Local-only bind | AuthN/AuthZ, mTLS, per-route authorization |
| State corruption on crash/disconnect | Append-only JSONL rebuilt by replay on open | SQLite WAL backend with integrity checks |

## Explicitly out of scope for v0
Authentication/authorization, transport security (TLS/mTLS), real cryptographic
signing/verification, secrets management, multi-tenant isolation, SQLite-backed
persistence, and any classified/real data. These are the first hardening items
when moving beyond prototype; the data shapes and trait boundaries above are
designed so adding them does not change the fusion contract.
