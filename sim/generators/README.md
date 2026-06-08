# Synthetic generators

The actual generator implementations live in the [`ingestion`](../../crates/ingestion)
crate (`ingestion::generators`) so they can be reused by both the live ingestion
path and the replay harness. They are deterministic given a seed:

| Family            | Generator                  | Native payload        | Normalized state |
|-------------------|----------------------------|-----------------------|------------------|
| Radar             | `RadarGenerator`           | range/bearing/elev    | yes (σ≈25 m)     |
| EO/IR             | `EoIrGenerator`            | bearing/elev/class    | yes (σ≈60 m)     |
| EW/SIGINT         | `EwEmitterGenerator`       | bearing/freq/strength | coarse (σ≈200 m) |
| Platform / PNT    | `PlatformStateGenerator`   | lat/lon/alt/heading   | yes (σ≈5 m)      |
| Operator console  | `OperatorOverrideGenerator`| directive             | none             |

## Scenarios

A scenario in [`../scenarios`](../scenarios) is a compact, declarative spec: it
names sources, their health, and one emitter per synthetic target over a
timeline. The replay harness (`replay::ReplayHarness`) expands the spec into a
concrete observation stream and runs it through a fresh fusion engine.

To run the bundled scenario through the engine and print metrics:

```bash
cargo test -p replay            # exercises scenario-01.json deterministically
```

To regenerate / author a new scenario, copy `../scenarios/scenario-01.json` and
adjust targets, seeds, and the timeline. Same seed ⇒ identical output.
