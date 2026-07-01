# Rust 编码规范

> 目标：写出让三个月后的自己和同事都能一眼看懂、安全、可测试的 Rust 代码，
> 并严守 DiPECS 的模块边界。
> 每条规则都有 **Why** 和 **Example**，而非罗列条款。

---

## 1. 总则

### 1.1 代码是写给人看的

编译器不关心变量名，但三个月后的你会关心。命名要精确，避免 `data`、
`info`、`item`、`value` 等泛化词。

```rust
// 差
let d = parse(s);
let r = execute(d);

// 好
let event = parse_raw_event(json);
let outcome = action_adapter.execute(&authorized);
```

### 1.2 利用编译器表达不变量

Rust 编译器是你最好的 reviewer。凡是能在编译期解决的问题，不要留到运行时。

- **优先使用类型系统表达不变量**，而非运行时断言。能用 `enum` 表达的状态机，
  就不要用 `bool` + `if`。
- **`clippy::pedantic` 不是噪声**。每条 lint 背后都是一个真实的 bug 模式。

### 1.3 一致性优于个人偏好

如果你在改一个已有文件，请保持和周围代码一致的风格。如果你发现整个项目都不一致，
提一个单独的 cleanup commit——不要把风格修正和功能改动混在同一个 commit 里。

### 1.4 严守模块边界

DiPECS 的 crate 依赖方向是：

```text
aios-spec -> aios-collector / aios-core / aios-agent
aios-core -> aios-action (ActionAdapter + AuthorizedAction)
aios-collector / aios-core / aios-agent / aios-action -> aios-daemon
```

禁止循环依赖。`aios-action` 对 `aios-core` 的依赖是 RFC-0002 为不可伪造性
`AuthorizedAction` deliberate 引入的例外，不能扩展为 action 读取 core 内部状态。

---

## 2. 命名

| 元素 | 约定 | 示例 |
| --- | --- | --- |
| 模块/文件名 | `snake_case` | `privacy_airgap.rs`, `action_lifecycle.rs` |
| 类型/特质/枚举变体 | `PascalCase` | `PrivacyAirGap`, `ActionState` |
| 函数/方法 | `snake_case` | `sanitize_with_tier`, `evaluate_batch` |
| 常量/静态变量 | `SCREAMING_SNAKE_CASE` | `DEFAULT_WINDOW_SECS` |
| 局部变量/参数 | `snake_case` | `raw_event`, `window_ordinal` |
| 布尔变量 | `is_`/`has_`/`should_` 前缀 | `is_charging`, `has_file_mention` |

### 2.1 命名要精确

```rust
// 差
let n = events.len();
let t = action.target();

// 好
let action_count = events.len();
let target = action.target();
```

### 2.2 缩写规则

- 极通用缩写可以用：`ctx`、`uid`、`pid`、`src`、`dst`。
- 避免自造的模棱两可缩写：`bin_nm`（请写 `binary_name`）、
  `cfg_pth`（请写 `config_path`）。

---

## 3. 格式化

### 3.1 自动化工具为唯一标准

- **使用 `cargo fmt` 处理所有格式问题**。不要手动调整缩进、换行、空格。
- 项目根目录的 `rustfmt.toml` 已将风格讨论收敛到单一配置。
- 每个 commit 前运行 `cargo fmt --all -- --check`。

### 3.2 行宽与换行

- `max_width = 100`。
- 如果某个表达式 `rustfmt` 换行后仍然难以阅读，那就说明它应该被提取为一个有名字的中间变量。

---

## 4. 代码组织

### 4.1 每个文件一个核心类型

一个 `.rs` 文件暴露一个核心类型或一个核心功能。与该类型紧密耦合的私有 helper
可以放在同一个文件中。

```text
crates/aios-core/src/
├── privacy_airgap.rs      # DefaultPrivacyAirGap
├── context_builder.rs     # WindowAggregator
├── policy_engine.rs       # PolicyEngine
├── action_lifecycle.rs    # ActionLifecycle
└── governance/
    ├── mod.rs             # AuthorizedAction, ActionAdapter
    └── ...
```

### 4.2 mod.rs 风格

推荐 `mod.rs` 风格，因为删除/移动目录时不会留下 orphan 文件。

```text
syscall/mod.rs
syscall/process.rs
```

### 4.3 可见性

- **默认 `pub(crate)`，而非 `pub`**。
- `pub` 是跨 crate API 承诺。每次你写 `pub`，就问自己：
  "如果这个签名要改，我会不会犹豫？"——如果会，它就不该是 `pub`。
- 不要为了测试把东西设为 `pub`。使用 `#[cfg(test)]` 模块直接访问 `pub(crate)` 项。

```rust
// 好：最小化公开面
pub(crate) struct AuthorizedAction {
    action: SuggestedAction,
    effect: EffectClass,
}
```

### 4.4 `use` 语句

- **禁止 `use crate::some::module::*`**（`*` 引入）。它让读者无法追踪符号来源。
- 对标准库和外部 crate，使用完整路径引入；对 crate 内部，优先 `use crate::parser::Parser`。
- `use` 语句放在文件顶部，按 `std` → 外部 crate → `crate` 三段分组，组间空一行。

```rust
use std::collections::HashMap;
use std::time::Duration;

use thiserror::Error;
use tracing::info;

use crate::context_builder::WindowAggregator;
use crate::privacy_airgap::DefaultPrivacyAirGap;
```

### 4.5 模块边界

- `aios-spec` 只放数据结构和协议 traits，零业务逻辑、零平台依赖。
- `aios-core` 是隐私与策略边界；`PrivacyAirGap`、`PolicyEngine`、
  `ActionLifecycle` 必须放在这里。
- `aios-action` 只执行 `AuthorizedAction`，不能自行 seal 授权动作。

---

## 5. 类型系统

### 5.1 enum >> bool

如果你有两个状态，今天就一个 `bool`。如果明天可能变成三个，今天就写 `enum`。

```rust
// 差
fn evaluate(context: &StructuredContext, use_cloud: bool) -> IntentBatch { ... }

// 好
enum DecisionRoute {
    RuleBased,
    LocalEvaluator,
    CloudLlm,
    FallbackNoOp,
}
```

### 5.2 善用 newtype

具有语义区别的基本类型，应包装为 newtype 避免混淆。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct WindowOrdinal(u32);
```

### 5.3 构造函数

每个 `struct` 应有一个明确的构造入口，通常是 `new()` 或 `with_*()`。
构造函数应 **构造** 对象，而非执行副作用（不读写文件、不发起网络请求）。

```rust
impl WindowAggregator {
    pub fn new(window_secs: u64, now_ms: i64) -> Self { ... }
}
```

---

## 6. 错误处理

### 6.1 库代码使用 `thiserror`

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("android bridge forward failed: {0}")]
    ForwardFailed(String),
    #[error("socket io error: {0}")]
    Io(#[from] std::io::Error),
}
```

**Why `thiserror`**：手写 `Display` + `Error` + `source` 的样板代码容易出错，
且当错误变体增加时维护成本高。

### 6.2 错误传播

- 使用 `?` 传播错误。不要写 `match` + `return Err(...)`。
- 应用层可用 `anyhow::Context` 为错误附加调用上下文。

```rust
let content = std::fs::read_to_string(path)
    .with_context(|| format!("failed to read trace: {}", path.display()))?;
```

### 6.3 `unwrap()` / `expect()` 使用标准

- **`unwrap()`**：库代码禁止。它不携带任何上下文信息。
- **`expect("why this can't fail")`**：仅在"逻辑上不可能失败"的位置使用，
  且消息必须解释 **为什么** 不可能，而非描述 **如果失败是什么错误**。
- 若确实逻辑不可达，用 `unreachable!()` 并加注释说明为什么外部输入触达不到。
- 测试代码除外。

```rust
// 差：消息只是描述了失败场景
let home = std::env::var("HOME").expect("HOME not set");

// 好：解释了为什么不可能失败
let home = std::env::var("HOME")
    .expect("POSIX requires HOME to be set for login shells");

// 更好：用 Result 传播
let home = std::env::var("HOME")
    .context("HOME environment variable not set")?;
```

### 6.4 系统调用错误的特殊处理

因为我们与 `libc` 交互，规则是：

- **不要吞掉 `errno`**。错误信息中必须包含 `errno` 的值和含义。
- 对裸 `libc` 调用，立即将返回值转为 `Result`，不要延迟到下一行。

```rust
// 差：errno 可能被中间调用覆盖
let ret = unsafe { libc::close(fd) };
if ret < 0 {
    let msg = format!("close({fd}) failed"); // errno 可能已经不是原来的了
}

// 好：立即保存 errno
let ret = unsafe { libc::close(fd) };
if ret < 0 {
    return Err(AdapterError::Io(std::io::Error::last_os_error()));
}
```

---

## 7. 所有权与借用

### 7.1 永远不要为了"方便"使用 `clone()`

如果你写 `.clone()` 只是为了不跟 borrow checker 打交道，停下来想三秒：

1. 能否改为借用（`&T`）？
2. 能否移交所有权（move）？
3. 能否用 `Rc`/`Arc` 共享？
4. 如果以上都不行——clone 也可以，但加一行注释说明原因。

```rust
// 差：只看一眼代码不知道为什么要 clone
let event = events[0].clone();
process(event);

// 好：借用即可
process(&events[0]);

// 可接受：有明确语义需求
let event = events[0].clone(); // Clone：需要把 event 移到后台线程
thread::spawn(move || process(event));
```

### 7.2 优先使用引用而非智能指针

`&T` > `Box<T>` > `Rc<T>` > `Arc<T>`。只在必要时提升。

### 7.3 生命周期标注

大多数场景下，生命周期省略规则能处理。如果手动标注 `'a`，
在函数签名上方加一行注释解释 `'a` 代表什么。

---

## 8. 文档与注释

### 8.1 注释哲学

代码告诉你 **做了什么**，注释告诉你 **为什么这么做**。

以下情况需要注释：

- 非显而易见的算法或数据结构选择。
- 某段代码存在的原因（workaround、边界条件）。
- `unsafe` 块的前置条件。
- 公开 API 的线程模型或调用前提。

以下情况不需要注释：

- 代码本身已经清晰表达的逻辑。
- 明显的 `// TODO`——要么修掉，要么提到 Issue 追踪。

### 8.2 Doc comment（`///` 和 `//!`）

规则：

1. **每个 `pub` 项都必须有 doc comment**（包括 `pub(crate)`）。
2. **首行是一个以句号结尾的单句摘要**。`rustdoc` 将其作为列表页面的描述。
3. **空行后写详细说明**。
4. **使用 Markdown 格式**。`` ` `` 包裹代码片段，```` ```rust ```` 包裹代码块。

```rust
/// Sanitize a raw event into a privacy-preserving representation.
///
/// This is the single point where PII must be dropped. Callers must not
/// pass the returned `SanitizedEvent` back into anything that could reconstruct
/// the original `RawEvent`.
pub fn sanitize(&self, event: &RawEvent) -> SanitizedEvent { ... }
```

### 8.3 `unsafe` 代码

- **默认不允许 `unsafe`**。在使用它之前，穷尽所有 safe 替代方案。
- 每次使用 `unsafe` 必须带 `// SAFETY:` 注释，说明：
  1. 为什么必须用 `unsafe`。
  2. 所有前置条件。
  3. 为什么这些条件可以由代码审查者在本地验证。

```rust
// SAFETY: `redirect_stdio_to_dev_null` 只在 daemonize 后的单进程上下文中调用，
// 此时 stdio 已关闭，打开的 `/dev/null` fd 是合法且唯一的。
let fd = unsafe { libc::dup(0) };
```

---

## 9. 测试

### 9.1 测试金字塔

```text
        /\  
       /E2E\         少量端到端（action-loop / emulator）
      /------\  
     / 集成  \        中量集成（replay / golden hash / pipeline）
    /----------\  
   /  单元测试   \     大量单元（单函数/单方法）
  /--------------\  
```

### 9.2 单元测试

- 放在被测试代码同一个文件中，包裹在 `#[cfg(test)] mod tests { ... }` 里。
- 测试函数命名：`test_<function_name>__<scenario>`（双下划线分隔函数名和场景）。
- 每个测试只验证一件事。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize__notification_drops_raw_text() {
        let event = RawEvent::NotificationPosted(...);
        let sanitized = DefaultPrivacyAirGap.sanitize(&event);
        assert!(!sanitized.has_raw_text());
    }
}
```

### 9.3 集成测试

放在 `tests/` 目录下。每个测试文件是一个独立的二进制，测试完整端到端流程。

DiPECS 的关键集成测试：

- `privacy_leak_test`：PII 不越过 `PrivacyAirGap`。
- `replay_golden_hash_test`：`audit_hash` 跨多次运行稳定。
- `policy_denial_golden_test`：每种 `DenialReason` 都被触发。
- `action_lifecycle_test`：每个 `ActionCoord` 恰好一条终态审计。

### 9.4 表驱动测试

对于纯数据转换（sanitizer、serializer），使用表驱动测试减少样板代码。

```rust
#[test]
fn test_sanitize_notification_text() {
    let cases = vec![
        ("Alice sent a file", vec![SemanticHint::FileMention]),
        ("Your code is 123456", vec![SemanticHint::VerificationCode]),
    ];
    for (raw, expected) in cases {
        let hints = extract_semantic_hints(raw);
        assert_eq!(hints, expected, "failed on input: {raw:?}");
    }
}
```

### 9.5 边界护栏

- 新增 `RawEvent`：补 `aios-spec` 类型、`PrivacyAirGap` 脱敏测试、窗口聚合测试。
- 新增决策规则：补后端测试，并说明后端能力上限。
- 新增动作：补 `PolicyEngine` 审查测试和 `aios-action` 执行结果测试。
- 涉及 action-loop 的变更需通过 mock-socket 验证。

---

## 10. 异步边界

DiPECS 的异步边界有明确分工：

- **`aios-core` 内部保持同步**。`PrivacyAirGap`、`PolicyEngine`、
  `ActionLifecycle` 都是纯函数或同步状态机。
- **异步只在 I/O 边界使用**：`aios-daemon` 的 tokio mpsc channel、
  `aios-action` 的 Android bridge TCP、HTTP 请求。
- **`aios-agent` 的 `CloudLlmBackend` 是异步**，因为它做网络 I/O；
  `RuleBasedBackend` 和 `LocalEvaluatorBackend` 保持同步。

```rust
// core：同步
pub fn evaluate_batch(&self, batch: &IntentBatch) -> Vec<PolicyActionDecision> { ... }

// adapter：异步 I/O
pub async fn forward(&self, action: &AuthorizedAction) -> Result<ActionOutcome, AdapterError> { ... }
```

---

## 11. 依赖管理

### 11.1 添加依赖前过三关

1. **标准库是否已经提供了？**
2. **依赖本身是否健康？**——检查最近更新时间、下载量、issue 数量。
3. **引入的 transitive dependencies 有多少？**——`cargo tree` 看看。

### 11.2 版本固定

- 可执行程序（binary crate）：`Cargo.lock` 必须提交到 git。
- 库（library crate）：在 `Cargo.toml` 中指定兼容版本范围，如 `"0.31"`
  （等同于 `>=0.31.0, <0.32.0`）。

### 11.3 Feature flag 最小化

- 默认关闭所有 optional feature。`default = []`。
- 每个 feature 在 `Cargo.toml` 上方加注释说明它引入的额外依赖和增加的编译时间。

```toml
# 启用云端 LLM provider（增加 reqwest + rustls，约 +2s 编译时间）
cloud-llm = ["reqwest", "rustls"]
```

---

## 12. 工程化工作流

### 12.1 提交前检查清单

每次开发完成一个小功能后执行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

### 12.2 Commit message

使用 Conventional Commits：

```text
feat(core): add capability check for local evaluator
fix(agent): route fallback when cloud misconfigured
docs(readme): clarify authorized action boundary
test(action): add android bridge envelope coverage
```

### 12.3 分支策略

- `main`：始终可编译、可通过测试。
- 功能开发：`feat/<short-name>`。
- Bug 修复：`fix/<short-name>`。
- 文档：`docs/<short-name>`。

---

## 13. 性能心态

### 13.1 正确性 > 简洁 > 性能

在你证明某段代码是性能瓶颈之前（通过 `perf`、`flamegraph` 或 `criterion` benchmark），
不要牺牲可读性去"优化"它。

### 13.2 对 DiPECS 的具体建议

- 事件流处理优先用 `&str` 切片而非频繁 `String` 分配。
- 已知大小的缓冲区用 `Vec::with_capacity` 预分配。
- 脱敏和聚合是纯数据转换，避免在 hot path 中做 I/O。

---

## 14. 快速决策速查表

| 场景 | 选择 |
| --- | --- |
| 两种以上状态 | `enum` |
| 函数可能失败 | `Result<T, E>` |
| 库错误类型 | `thiserror::Error` |
| 应用层附加上下文 | `anyhow::Context` |
| 类型语义不同 | newtype wrapper |
| 公开 API | doc comment + examples |
| `unsafe` | `// SAFETY:` 注释 |
| 默认可见性 | `pub(crate)` |
| 提交前 | fmt + clippy + test 全绿 |
| 分支名 | `feat/` / `fix/` / `docs/` |
| 字符串参数 | `&str`（不是 `String`） |

---

## 参考资料

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Book - Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/)
