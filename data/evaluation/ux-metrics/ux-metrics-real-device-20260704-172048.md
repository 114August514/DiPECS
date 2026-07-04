# DiPECS Real-Device UX Metrics Measurement

- Dataset: `ux-metrics-real-device-20260704-172048.json`
- Status: measured on Pixel 6a (bluejay), Android 16
- Sample interval: 5 seconds
- Samples per mode: 5

## Startup Latency (am start -W TotalTime)

| Mode | TotalTime avg | TotalTime p95 | RSS avg | PSS avg |
| --- | ---: | ---: | ---: | ---: |
| cold_startup | 600.4 ms | 620.0 ms | 210.5 MB | 93.659 MB |
| prewarm_startup | 142.6 ms | 168.0 ms | 218.825 MB | 101.261 MB |

**PreWarm effect:** 457.8 ms faster (76.2%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 4.76% | 206.859 MB | 87.42 MB |
| post_release_jank | 4.76% | 186.397 MB | 67.002 MB |

**ReleaseMemory effect:** jank 0.0 pp, PSS 20.418 MB

**ReleaseMemory interpretation:** neutral_idle_no_jank_improvement

## Conclusion

- PreWarm effective: True
- ReleaseMemory effective: False
- ReleaseMemory status: neutral_idle_no_jank_improvement
