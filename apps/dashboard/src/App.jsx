import { useEffect, useState, useCallback } from "react";

// Minimal, read-only operator view. It does not decide anything — it surfaces
// fused tracks, their confidence and reason codes, preserved conflicts, and the
// evaluation metrics from the last replay. All data comes from the fusion-api.

async function getJSON(url, opts) {
  const res = await fetch(url, opts);
  if (!res.ok) throw new Error(`${url} → ${res.status}`);
  return res.json();
}

function ConfidenceBar({ score }) {
  const pct = Math.round((score ?? 0) * 100);
  const color = pct >= 70 ? "var(--ok)" : pct >= 45 ? "var(--caution)" : "var(--warning)";
  return (
    <div title={`${pct}%`}>
      <div className="bar">
        <span style={{ width: `${pct}%`, background: color }} />
      </div>
      <span className="muted">{score?.toFixed(3)}</span>
    </div>
  );
}

export default function App() {
  const [health, setHealth] = useState(null);
  const [tracks, setTracks] = useState([]);
  const [metrics, setMetrics] = useState([]);
  const [session, setSession] = useState(null);
  const [error, setError] = useState(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [h, t, m] = await Promise.all([
        getJSON("/health"),
        getJSON("/api/v1/tracks"),
        getJSON("/api/v1/metrics"),
      ]);
      setHealth(h);
      setTracks(t);
      setMetrics(m.metrics || []);
      setSession(m.session || null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 2000);
    return () => clearInterval(id);
  }, [refresh]);

  const runReplay = async () => {
    setBusy(true);
    try {
      await getJSON("/api/v1/replay/start", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ scenario: "scenario-01" }),
      });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="app">
      <header>
        <h1>Assured Edge Fusion — Operator View</h1>
        <span className="subtitle">
          {health ? `api ${health.version} · schema v${health.schema_version} · up ${Math.round(health.uptime_ms / 1000)}s` : "connecting…"}
        </span>
      </header>

      {error && <p className="err">⚠ {error} — is fusion-api running on :8088?</p>}

      <div className="row">
        <div className="panel" style={{ flex: 2 }}>
          <h2>Fused tracks (live engine)</h2>
          {tracks.length === 0 ? (
            <p className="muted">No live tracks. Submit observations to the API, or run a replay (replay uses an isolated engine).</p>
          ) : (
            <table>
              <thead>
                <tr><th>Track</th><th>Status</th><th>Confidence</th><th>Reasons</th><th>Conflicts</th><th>Sources</th></tr>
              </thead>
              <tbody>
                {tracks.map((t) => (
                  <tr key={t.track_id}>
                    <td>{t.track_id}<span className="muted"> v{t.version}</span></td>
                    <td className={`status-${t.status}`}>{t.status}</td>
                    <td style={{ minWidth: 110 }}><ConfidenceBar score={t.confidence?.score} /></td>
                    <td>{(t.confidence?.reason_codes || []).map((r) => <span className="pill" key={r}>{r}</span>)}</td>
                    <td className={t.conflicts?.length ? "sev-warning" : "muted"}>{t.conflicts?.length || 0}</td>
                    <td className="muted">{(t.contributing_sources || []).join(", ")}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <div className="panel">
          <h2>Evaluation metrics</h2>
          <button onClick={runReplay} disabled={busy}>{busy ? "running…" : "Run replay: scenario-01"}</button>
          {session && <p className="muted" style={{ marginTop: 8 }}>{session.scenario_name} · {session.event_count} events · {session.status}</p>}
          <div style={{ marginTop: 8 }}>
            {metrics.length === 0 ? (
              <p className="muted">No replay run yet.</p>
            ) : (
              metrics.map((m) => (
                <div className="metric" key={m.metric}>
                  <span>{m.metric}</span>
                  <span>{m.value.toFixed(3)} <span className="muted">{m.unit}</span></span>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
