# DiPECS 测试入口

本目录放仓库级集成测试和端到端场景脚本。普通单元测试仍放在对应 crate 或
Android app 模块旁边；当测试跨模块、跨二进制，或需要 Android/模拟器目标时，
再放到这里。

## 目录结构

| 路径 | 用途 |
| --- | --- |
| `integration/` | Rust integration-test crate，用于 baseline 对照和跨模块行为测试。 |
| `scenarios/` | Shell 驱动的 Android/模拟器端到端脚本，负责安装、启动、端口转发、动作派发和证据采集。 |
| `scenarios/lib/` | 场景脚本共享的 shell/Python helper；除 `*-selftest.sh` 外通常不是独立测试入口。 |

## Rust 集成测试

`tests/integration` 是独立的 Cargo test crate，覆盖 policy denial、routing
strategy、NoOp coverage、window size、Cloud LLM stability、action success
rate、HMAC cross-verification 和 rationale coverage 等 baseline / 跨模块检查。

运行全部 integration 模块：

```bash
cargo test --test integration
```

运行单个模块：

```bash
cargo test --test integration policy_denial
```

当前模块列表见 `tests/integration/README.md`。

## Android 场景脚本

场景脚本是手动或类 CI 的端到端检查。通常需要 Android SDK、`adb`、已构建/安装
的 collector app；部分脚本还需要模拟器、root 或 userdebug 设备。

| 脚本 | 使用场景 |
| --- | --- |
| `scenarios/emulator-e2e.sh` | 在模拟器上验证 Android public-API 采集、JSONL 导出、Rust replay 和 audit hash。 |
| `scenarios/action-loop-e2e.sh` | 验证 host daemon -> adb forward -> Android action socket -> handler/audit 的动作回路。 |
| `scenarios/action-latency-sweep.sh` | 测量支持动作的设备侧确认延迟。 |
| `scenarios/on-device-dipecsd.sh` | 验证设备内 `dipecsd` 经 loopback 直连 app action socket。 |
| `scenarios/real-device-validation-console.sh` | 交互式真机验证辅助脚本，用于权限、trace preview 和 action bridge 检查。 |

常用入口：

```bash
bash tests/scenarios/emulator-e2e.sh --auto
bash tests/scenarios/action-loop-e2e.sh
bash tests/scenarios/on-device-dipecsd.sh
```

多数场景脚本会把结构化证据写入 `data/evaluation/`。不要提交原始私人设备数据、
action socket token、未脱敏通知、私有文件名或用户相关 cache path。

## 场景共享 helper

`tests/scenarios/lib` 放可复用 stage 和 probe：

| Helper | 用途 |
| --- | --- |
| `action-forensic-sender.py` | 向 Android action socket 发送签名 execute envelope，用于取证/手动检查。 |
| `action-loop-stages.sh` | action-loop 的 setup、adb forward、状态分类和 artifact helper。 |
| `emulator-e2e-stages.sh` | emulator collection / replay 的共享 stage。 |
| `on-device-dipecsd-stages.sh` | on-device daemon setup / validation 的共享 stage。 |
| `*-selftest.sh` | 对应 stage helper 的快速自测。 |

## 怎么选择入口

- 纯 Rust、policy 或 routing 变更：先跑 `cargo test --workspace`；若影响 baseline
  行为，再跑 `cargo test --test integration`。
- Android action bridge、HMAC、socket 或 handler 变更：先跑对应 crate 测试，再在
  adb target 上跑 `tests/scenarios/action-loop-e2e.sh`。
- Android 采集变更：跑 `tests/scenarios/emulator-e2e.sh --auto`。
- 设备内 daemon 变更：确认目标架构和所需权限后，跑
  `tests/scenarios/on-device-dipecsd.sh`。

如果场景需要物理设备，先用 `adb devices -l` 确认目标设备；除非脚本或实验明确要求，
不要执行 root/system-level 操作。
