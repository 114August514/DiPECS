# DiPECS Emulator UX Metrics Measurement

- Dataset: `ux-metrics-emulator-20260703-171457.json`
- Status: measured on Android Studio emulator
- Sample interval: 10 seconds
- Samples per mode: 10

## Startup Latency (am start -W TotalTime)

| Mode | TotalTime avg | TotalTime p95 | RSS avg | PSS avg |
| --- | ---: | ---: | ---: | ---: |
| cold_startup | 884.1 ms | 932.0 ms | 165.58 MB | 67.333 MB |
| prewarm_startup | 489.3 ms | 512.0 ms | 164.411 MB | 65.867 MB |

**PreWarm effect:** 394.8 ms faster (44.7%)

## Jank / Memory (dumpsys gfxinfo + meminfo)

| Mode | Avg jank | Avg RSS | Avg PSS |
| --- | ---: | ---: | ---: |
| baseline_jank | 23.81% | 153.612 MB | 53.501 MB |
| post_release_jank | 23.81% | 153.758 MB | 53.963 MB |

**ReleaseMemory effect:** jank 0.0 pp, PSS -0.462 MB

**ReleaseMemory interpretation:** neutral in this idle emulator run; not cited as a positive benefit until memory-pressure retest.

## Conclusion

- PreWarm effective: True
- ReleaseMemory effective: False
- ReleaseMemory status: neutral_idle_no_jank_improvement
