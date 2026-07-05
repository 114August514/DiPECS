# PrefetchFile Benefit Evidence (#97)

`PrefetchFile` is not considered complete just because Android accepts the
action. Issue #97 requires measured benefit evidence:

- at least `n>=20` samples per measured mode;
- mean and p95 latency;
- separate hit and miss costs;
- same-budget comparison against `StrongPredictiveActionBaseline`;
- measured provenance from an Android adb target.

## Collection

Run the collector app with the action socket enabled, then collect:

```bash
SAMPLES=20 \
PREFETCH_URL=https://raw.githubusercontent.com/114August514/DiPECS/main/README.md \
EXAMPLES=<test-window-count> \
DIPECS_HIT_RATE_PCT=<dipecs-hit-rate> \
STRONG_HIT_RATE_PCT=<strong-baseline-hit-rate> \
tools/collect/collect-prefetch-file-benefit.sh
```

The script writes JSON and Markdown artifacts under
`data/evaluation/action-net-benefit/`.

## What It Measures

The script measures two modes:

| Mode | Meaning |
| --- | --- |
| `prefetched_read` | Clear the prefetch cache, execute `PrefetchFile`, wait for the cache file, then read the cached file once with `run-as`. |
| `miss_fetch_then_read` | Clear the cache, execute `PrefetchFile`, wait for the file to be downloaded, then read it once. |

`hit_saved_ms` is derived from the miss end-to-end cost minus the cached read
cost. `miss_action_cost_ms` is the measured prefetch wait cost. Dispatch latency
is recorded as `control_plane_ms`.

## Acceptance

The generated artifact is accepted only when all gates are true:

- `n_at_least_20_per_mode`;
- `measured_inputs_valid`;
- `same_budget_baseline_inputs_present`;
- `net_benefit_positive`;
- `dipecs_beats_strong_predictive`.

If the same-budget hit-rate inputs are omitted, the script still produces a
measurement artifact, but it remains `measurement_pending_baseline_gate` and
must not be cited as closing #97.

## Pixel 6a Status (2026-07-05)

Pixel 6a (`2B071JEGR05551`) currently cannot produce an accepted #97 artifact.
The bridge accepts `PrefetchFile` envelopes after aligning the sender timestamp
to the device clock, but the asynchronous Android prefetcher does not produce a
cache file:

- `url:https://raw.githubusercontent.com/114August514/DiPECS/main/README.md`
  returned `prefetch_failed` and no `prefetch_succeeded`;
- `url:https://example.com/`, `url:https://badssl.com/`, and
  `url:https://www.google.com/generate_204` also failed to populate
  `cache/prefetch`;
- `uri:content://media/external/downloads/427` failed without a SAF-granted
  persisted read permission; a scripted DocumentsUI grant attempt was not stable
  enough to use as evidence.

The device system clock is still `2025-06-07` while the host date is
`2026-07-05`; HTTPS failures are therefore likely affected by TLS validity
checks, but changing the device-wide clock with root was not performed because
it is a persistent global device mutation. Until the prefetcher reaches
`prefetch_succeeded` and creates a cache file, do not run or cite the n>=20
benefit gate for #97.
