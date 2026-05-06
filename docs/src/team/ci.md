# DiPECS CI 自动化质检体系

DiPECS 跨 Android 内核、自研协议和 AI 逻辑，CI 必须建立多层防御体系，拦截一切会导致系统状态"退化"的提交。

---

## 流水线全景

| 流水线 | 触发频率 | 核心目标 | 关键任务 |
|:---|:---|:---|:---|
| **Lint** | 每次推送 | 语法合规 | `fmt`, `clippy`, `cargo-machete` |
| **Test** | 每次 PR | 逻辑正确性 | `nextest`, `replay` (逻辑回放) |
| **Build** | 每天 / PR | 跨平台验证 | `android-cross-build`, `x86-ubuntu-build` |
| **Security** | 每周 / PR | 系统鲁棒性 | `cargo-audit`, `cargo-deny` |
| **Bench** | 合并前 | 零退化保证 | `cargo-bloat`, `criterion` |
| **Docs** | 合并后 | 知识对齐 | `cargo-doc`, `mdBook` → GitHub Pages |

---

## 五道自动关卡

### 1. 语法与静态语义关卡 (Hygiene Gate)

成本最低、反馈最快。只做静态检查，不执行代码，不需要 LFS / NDK。

- **fmt**: 强制代码风格统一，不合规直接打回
- **clippy**: 检查潜在 Bug、低效写法、`unsafe` 滥用
- **cargo check**: 快速验证全平台类型系统闭环
- **cargo-machete**: 检查未使用依赖，防止引入无用 Crate

### 2. 物理构建关卡 (Build Gate)

确保在所有目标平台上能编过。很多代码 Ubuntu 能编过，但 Android NDK 下因链接器符号缺失崩溃。

- **Ubuntu x86_64 Build**: 开发环境构建
- **Android aarch64 Cross-Build**: NDK 交叉编译，只跑 Release 编译，不跑测试。只要能产出 `aios-cli` 和 `libaios_kernel.so` 就算过。
- **多版本 NDK 兼容矩阵** (后续): 通过 Strategy Matrix 同时验证多 API Level

### 3. 逻辑正确性关卡 (Correctness Gate)

验证"状态转移"是否符合预期。

- **Unit Tests**: 基础原子测试
- **Integration Tests**: 跨 Crate 串联
- **Spec Integrity**: 验证序列化/反序列化的向前兼容性
- **Deterministic Trace Replay** (核心): CI 从 `data/traces/` 取 Golden Traces，回放验证 Action 序列是否一致。如果算法优化导致"动作漂移"，CI 必须报警。

### 4. 资源约束关卡 (Resource Gate)

防止系统变臃肿。

- **Binary Size Audit (`cargo-bloat`)**: 监控 `.so` 体积。PR 让二进制增加超过 100KB 时，在 PR 下自动评论提醒。
- **Dependency Audit (`cargo-deny`)**: 检查依赖库的许可证合规性和未使用依赖。建议从 `test.yml` 独立出来放入 `audit.yml`。

### 5. 安全与隐私关卡 (Security Gate)

- **漏洞审计 (`cargo-audit`)**: 每周运行，检查依赖是否存在已知 CVE
- **隐私泄漏测试**: 验证脱敏逻辑正确拦截敏感信息
- **Secret Scanning**: 检查代码中是否误提交了云端 API Key

---

## 执行策略 (分层触发)

1. **Level 1 — 提交即触发 (3min)**: `fmt` + `clippy` + `cargo check` (Ubuntu)
2. **Level 2 — PR 必备 (8min)**: `cargo test` + Android Cross-Build
3. **Level 3 — 合并前最后验证 (15min)**: Trace Replay + `cargo-bloat` + Security Scan

---

## 后续补充任务（优先级）

1. **Security Audit** — 几行配置即可保护系统不受 CVE 侵害
2. **Trace Replay** — 系统确定性的命脉
3. **Binary Size Audit** — 物理层面的紧迫感
