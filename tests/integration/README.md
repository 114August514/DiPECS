# DiPECS Baseline Integration Tests

本 crate 用于集中存放 DiPECS 各维度的 baseline（对照组）集成测试。
每个模块对应一个 baseline 维度，后续 Task 只需在对应模块里补充具体测试用例。

## 运行方式

```bash
# 编译并运行全部 baseline 测试
cargo test --test integration

# 只运行单个 baseline 模块
cargo test --test integration policy_denial
```

## Baseline 模块

| 模块 | 维度 |
| --- | --- |
| `policy_denial` | PolicyEngine 拒绝率 |
| `routing_strategy` | 决策路由策略 |
| `noop_coverage` | NoOp 覆盖率 |
| `window_size` | 窗口大小 |
| `cloud_llm_stability` | 云端 LLM 稳定性 |
| `action_success_rate` | 动作执行成功率 |
| `signature_cross_verify` | HMAC 签名交叉验证 |
| `rationale_coverage` | 决策理由标签覆盖率 |

## 共享工具

`helpers.rs` 提供加载 JSONL trace、定位仓库根目录等通用辅助函数。
