# 测试策略

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `crates/*/tests/`, `tests/scenarios/`, `.github/workflows/test.yml`

**这篇文档回答什么**：DiPECS 有哪些测试层、每层验证什么、需要什么前置条件。  
**适合谁读**：需要决定跑哪些测试、新增测试或排查 CI 失败的人。

## TL;DR

测试分四层：

1. **单元/集成测试**：`cargo test --workspace`，无需 Android/API key。
2. **Dataset 回归测试**：基于已提交 fixture 验证资源/UX/稳定性阈值。
3. **Mock-socket 测试**：本地 TCP 模拟 Android bridge。
4. **端到端脚本**：模拟器/真机场景。

## 测试层总览

| 层 | 命令 | 前置条件 |
| --- | --- | --- |
| 单元 + crate 集成 | `cargo test --workspace` | Rust toolchain |
| Dataset 回归 | `cargo test -p aios-cli --test resource_overhead_dataset_test` 等 | fixture JSON |
| Mock-socket | `cargo test -p aios-action android_bridge_e2e_test` | Rust only |
| Emulator E2E | `bash tests/scenarios/emulator-e2e.sh --auto` | Android SDK、模拟器 |
| Action-loop E2E | `bash tests/scenarios/action-loop-e2e.sh` | 真机/模拟器 |
| On-device | `bash tests/scenarios/on-device-dipecsd.sh` | NDK、root/userdebug 设备 |

## 关键集成测试

| 测试 | 文件 | 验证点 |
| --- | --- | --- |
| `privacy_leak_test` | `crates/aios-core/tests/privacy_leak_test.rs` | PII 不越过 `PrivacyAirGap` |
| `privacy_airgap_property_test` | `crates/aios-core/tests/privacy_airgap_property_test.rs` | 生成式隐私检查 |
| `replay_golden_hash_test` | `crates/aios-cli/tests/replay_golden_hash_test.rs` | `audit_hash` 稳定 |
| `policy_denial_golden_test` | `crates/aios-core/tests/policy_denial_golden_test.rs` | 每个 `DenialReason` 都被触发 |
| `android_bridge_e2e_test` | `crates/aios-action/tests/android_bridge_e2e_test.rs` | HMAC、freshness、执行路径 |
| `android_adapter_test` | `crates/aios-cli/tests/android_adapter_test.rs` | bridge 失败诚实映射 |
| `action_lifecycle_test` | `crates/aios-core/tests/action_lifecycle_test.rs` | 每个 `ActionCoord` 一条终态审计 |

## Dataset 测试

| 测试 | fixture | 阈值 |
| --- | --- | --- |
| `resource_overhead_dataset_test` | `data/evaluation/resource-overhead-emulator-*.json` | CPU delta ≤ 8 pp，PSS delta ≤ 80 MB |
| `ux_metrics_dataset_test` | `data/evaluation/ux-metrics-emulator-*.json` | PreWarm 加速 ≥ 20% 或 ≥ 100 ms；jank 增加 ≤ 20 pp |
| `stability_dataset_test` | `data/evaluation/stability-emulator-canonical.json` | RSS ≤ 50 MB/h，PSS ≤ 20 MB/h，CPU ≤ 10% |

## 场景脚本

| 脚本 | 目的 | 产物 |
| --- | --- | --- |
| `emulator-e2e.sh` | 模拟器采集 + replay | `data/evaluation/emulator-e2e-*.md` |
| `action-loop-e2e.sh` | host daemon 通过 adb forward 发 action | `data/evaluation/action-loop-e2e-*.md` |
| `action-latency-sweep.sh` | 四种 action 延迟扫描 | 终端输出 |
| `on-device-dipecsd.sh` | 设备内运行 dipecsd | `data/evaluation/on-device-dipecsd-*.md` |

## 需要 API key 的测试

云端 LLM 测试默认 `#[ignore]`，需要：

```bash
DIPECS_CLOUD_LLM_ENABLED=true \
DIPECS_CLOUD_LLM_PROVIDER=deepseek \
DIPECS_CLOUD_LLM_API_KEY=<key> \
cargo test -p aios-agent --lib cloud_llm::cloud_bench_tests::smoke -- --ignored
```

## 新增测试 checklist

- [ ] 新增 `RawEvent`：补 `aios-spec` 类型 + 脱敏测试 + 聚合测试
- [ ] 新增决策规则：补后端测试
- [ ] 新增动作：补 `PolicyEngine` 审查 + `aios-action` 执行测试
- [ ] action-loop 变更：通过 mock-socket 测试
- [ ] 新增评估指标：补 dataset test 和 fixture

## 相关文档

- [评估场景与数据集](../evaluation/scenarios.md)
- [评估工具](../evaluation/tools.md)
- [调试指南](../team/debugging.md)
- [CI 质检体系](../team/ci.md)
