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

Pixel 6a (`2B071JEGR05551`) produced an accepted #97 artifact:

- JSON: `data/evaluation/action-net-benefit/prefetch-file-benefit-20260705-201053.json`;
- Markdown: `data/evaluation/action-net-benefit/prefetch-file-benefit-20260705-201053.md`;
- target:
  `url:https://raw.githubusercontent.com/114August514/DiPECS/main/docs/src/slides/final/package-lock.json`;
- payload size: 399,165 bytes in every sample;
- samples: n=20 for `prefetched_read`, n=20 for `miss_fetch_then_read`;
- prefetched read mean/p95: 79.993/101.332 ms;
- miss fetch+read mean/p95: 1860.332/2276.297 ms;
- measured per-hit saved latency: 1780.339 ms;
- measured miss action cost: 1775.8 ms;
- dispatch/control cost: 8.916 ms/action.

Same-budget LSApp standard inputs were provided from
`data/evaluation/next-app/lsapp-standard.report.json`:

- examples: 272,519;
- DiPECS ensemble hit@1: 56.509%;
- `StrongPredictiveActionBaseline` hit@1: 53.784%.

With those inputs, the projected net-benefit gate is positive and beats the
strong baseline:

- DiPECS projected net benefit: 61,268,324.531 ms;
- StrongPredictive projected net benefit: 34,859,928.678 ms;
- DiPECS minus strong baseline: +26,408,395.853 ms.

The accepted semantics are intentionally narrow: the device directly measures
per-hit saving and miss cost; the same-budget DiPECS-vs-strong comparison is
projected from real LSApp hit rates, not measured as two separate device traces.

### Device Clock Root Cause

The initial Pixel 6a failures were not caused by the phone hotspot. The device
could route and resolve external hosts (`8.8.8.8` and `raw.githubusercontent.com`
pinged successfully), and `com.dipecs.collector` had `INTERNET` permission.
The failing layer was HTTPS in the app process: the device clock was stuck at
`2025-06-07` while the host date was `2026-07-05`, and Android
`time_detector` had empty network and telephony suggestion histories even with
auto time enabled.

After setting the rooted test device clock to the host time, the same
`PrefetchFile` URL reached `prefetch_succeeded` and populated
`cache/prefetch`. The collection script still computes and exports
`DEVICE_CLOCK_OFFSET_MS` for bridge freshness, but HTTPS itself requires the
device wall clock to be plausible for certificate validation.

### Collector Fixes From The Run

The #97 run exposed two collector-side issues that are now fixed:

- `adb shell run-as "$PACKAGE" sh -c ...` must be sent as one remote command
  string; otherwise Android `adb shell` can split the intended `sh -c` command
  and make cache checks fail even when the file exists.
- The collector must wait for cache file size to stabilize before reading.
  The Android prefetcher writes directly to the final cache file, so waiting
  only for a non-empty file can read a partial download.
