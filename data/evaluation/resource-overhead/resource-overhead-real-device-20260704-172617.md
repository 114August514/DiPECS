# DiPECS Real-Device Resource Overhead Measurement

- Dataset: `resource-overhead-real-device-20260704-172617.json`
- Status: measured on Pixel 6a (bluejay), Android 16
- Sample interval: 5 seconds
- Samples per mode: 5
- CPU note: median of 3 adb top sub-samples per sample; near-zero or negative deltas are below measurement precision and should not be cited as exact CPU usage.
- Battery/thermal note: battery percentage and thermal sensors stayed flat during this short real-device run, so report-facing battery and thermal values below use the clearly marked estimate derived from measured CPU/PSS deltas.

| Mode | Avg CPU | Avg RSS | Avg PSS | Estimated battery drain | Estimated thermal delta | Avg jank |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline_idle | 0.0% | 0.0 MB | 0.0 MB | 0 mAh/min | 0 C | 0.0% |
| dipecs_observe_only | 0.0% | 134.411 MB | 38.946 MB | 0.042 mAh/min | 0.17 C | 0.0% |
| dipecs_action_loop | 0.0% | 136.956 MB | 40.726 MB | 0.109 mAh/min | 0.45 C | 0.0% |

## Estimate Basis

The device's raw battery percentage and thermal sensor stayed flat. To avoid reporting a misleading `0%` power result, the table above combines measured CPU/RSS/PSS/jank with estimated battery and thermal values.
