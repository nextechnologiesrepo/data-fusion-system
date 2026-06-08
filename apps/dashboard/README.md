# Operator dashboard (minimal)

A deliberately minimal, **read-only** operator view. It is a replaceable edge
component, not the product — it just renders what `fusion-api` exposes: live fused
tracks with confidence/reason-codes/conflicts, and the evaluation metrics from the
last replay.

## Run

```bash
# 1. start the API (from repo root)
cargo run -p api

# 2. start the dashboard
cd apps/dashboard
npm install
npm run dev          # http://localhost:5173
```

The dev server proxies `/api`, `/health`, and `/openapi.yaml` to the API
(`VITE_API_TARGET`, default `http://localhost:8088`), so the browser only talks to
the dashboard origin.

Or run the whole stack with Docker: `docker compose up` from the repo root.
