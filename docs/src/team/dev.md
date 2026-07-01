# 开发指南

## 环境准备

详见 [环境配置指南](environment.md)。

必装：

- Rust 1.95.0（`rust-toolchain.toml` 锁定）
- 系统构建依赖（build-essential、lld 等）

按需：

- Android NDK r27d（交叉编译）
- MkDocs Material（文档构建）

## 项目结构

```text
DiPECS/
├── crates/              # Rust 核心模块
│   ├── aios-spec/       # 宪法层：数据类型 + Trait
│   ├── aios-core/       # 逻辑层：状态机、策略、脱敏、聚合
│   ├── aios-action/     # 动作层：授权动作执行
│   ├── aios-collector/  # 采集层入口：app/system 来源 -> RawEvent
│   ├── aios-agent/      # 决策层：DecisionRouter + 模型后端
│   ├── aios-daemon/     # 编排层：dipecsd 长驻进程入口
│   └── aios-cli/        # 命令行工具
├── apps/                # Android 应用
├── docs/                # 文档（MkDocs + academic-src）
├── data/traces/         # Golden Traces
└── scripts/             # 自动化脚本
```

## 构建

```bash
# 本地开发
cargo build --workspace

# Release 构建
cargo build --workspace --release

# Android 交叉编译
cargo build --target aarch64-linux-android

# 文档
cd docs
PYTHONPATH=. uv run mkdocs serve
```

## 提交前检查

```bash
# 一键全量检查
./scripts/check-all.sh

# 或逐项执行：
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

## 调试工作流

### 运行 daemon（开发模式）

```bash
# 直接在宿主机运行（使用当前 Linux /proc 和系统状态；Binder probe 不可用时返回空）
cargo run -p aios-daemon --bin dipecsd -- --no-daemon

# 查看结构化日志
RUST_LOG=dipecs=debug cargo run -p aios-daemon --bin dipecsd -- --no-daemon
```

### 验证 core 行为

```bash
# Golden Trace 体系仍在补齐中；当前先跑 core 单元测试验证脱敏、聚合和策略语义
cargo test -p aios-core -- --nocapture
```

### 观察 daemon 内部状态

```bash
# tracing 输出所有管道事件
RUST_LOG=dipecs=trace cargo run -p aios-daemon --bin dipecsd -- --no-daemon 2>&1 | grep -E "sanitize|window|policy|execute"

# 统计各阶段延迟
RUST_LOG=dipecs=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon 2>&1 | grep "latency"
```

### Android 设备部署

```bash
# 交叉编译
source scripts/setup-env.sh
cargo build --target aarch64-linux-android --release

# 推送到模拟器/设备
adb push target/aarch64-linux-android/release/dipecsd /data/local/tmp/
adb shell chmod +x /data/local/tmp/dipecsd

# 运行（需要 root）
adb shell su -c "/data/local/tmp/dipecsd --no-daemon --verbose"

# 查看日志
adb logcat -s dipecs
```

## 添加新的数据源

1. 在 `aios-spec/src/event.rs` 定义新的 `RawEvent` 变体
2. 在 `aios-spec/src/sanitized.rs` 定义对应的 `SanitizedEvent` 变体
3. 在 `aios-core` 的 `PrivacyAirGap` 实现脱敏规则
4. 在 `aios-collector` 添加采集逻辑
5. 添加测试（参考 `privacy_airgap_test.rs` 结构验证、`privacy_leak_test.rs` JSON 子串扫描、`privacy_airgap_property_test.rs` 属性测试）

## 添加新的 Skill / Action

1. 在 `aios-spec/src/intent.rs` 定义新的 `IntentType` 和 `ActionType`
2. 在 `aios-agent/src/backends/rule_based.rs` 添加触发规则, 或新增实现 `DecisionBackend` 的后端
3. 在 `aios-action/src/lib.rs` 的 `DefaultActionExecutor::execute()` 添加执行分支
4. 更新 `policy_engine_test.rs` 覆盖新动作的风险等级

## 常见问题

**Q: `cargo test` 全部通过但 daemon 不工作？**
A: 测试覆盖了各模块的单元行为，但 daemon 需要 tokio runtime 启动。检查 `RUST_LOG=debug` 输出，确认 mpsc channel 没有提前 drop。

**Q: Android 交叉编译报链接错误？**
A: 确认 `scripts/setup-env.sh` 已执行，`ANDROID_NDK_HOME` 指向 NDK r27d，且 `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER` 已被设置。检查 `.cargo/config.toml` 的 target 配置。

**Q: Golden Trace 回放不一致？**
A: PrivacyAirGap 必须是纯函数——相同 RawEvent 输入必须产生相同 SanitizedEvent 输出。检查是否有非确定性来源（时间戳、UUID 生成），这些应使用 trace 中记录的值而非实时生成。
