# DiPECS Real-Device Action Latency Measurement

- Dataset: `action-latency-real-device-20260704-172936.json`
- Status: measured on Pixel 6a (bluejay), Android 16
- Script: `tests/scenarios/action-latency-sweep.sh`
- Bridge: `adb forward tcp:46321 tcp:46321`

| Action | Target | Device latency |
| --- | --- | ---: |
| KeepAlive | `work:collector_heartbeat` | 973 us |
| ReleaseMemory | `cache:prefetch` | 971 us |
| PreWarmProcess | `own:warmup` | 841 us |
| PrefetchFile | `url:https://example.com/` | 1964 us |

`device_latency_us` is reported by `AuthorizedActionSocketServer` and measures device-side time from accept through dispatch/ack. `PrefetchFile` acknowledgement means the prefetch was accepted/queued, not necessarily that remote network fetch completed.
