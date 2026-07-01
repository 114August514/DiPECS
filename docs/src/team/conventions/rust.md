# Rust 编码规范

> 目标：写出让三个月后的自己和同事都能一眼看懂、安全、可测试的 Rust 代码，
> 并严守 DiPECS 的模块边界。

---

## 1. 总则

### 1.1 代码是写给人看的

编译器不关心变量名，但三个月后的你会关心。命名要精确，避免 `data`、`info`、
`item` 等泛化词。

### 1.2 利用编译器表达不变量

能用 `enum` 表达的状态机，就不要用 `bool` + `if`。优先把业务不变量落到类型系统，
而非运行时断言。

### 1.3 严守模块边界

DiPECS 的 crate 依赖方向是：

```text
aios-spec -> aios-collector / aios-core / aios-agent
aios-core -> aios-action (ActionAdapter + AuthorizedAction)
aios-collector / aios-core / aios-agent / aios-action -> aios-daemon
```

禁止循环依赖。`aios-action` 对 `aios-core` 的依赖是 RFC-0002 为不可伪造性
`AuthorizedAction`  deliberate 引入的例外，不能扩展为 action 读取 core 内部状态。

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

---

## 3. 格式化

- 以 `cargo fmt` 为唯一标准。项目已配置 `max_width = 100`。
- 每个 commit 前运行 `cargo fmt --all -- --check`。

---

## 4. 代码组织

### 4.1 可见性

- **默认 `pub(crate)`，而非 `pub`**。
- `pub` 是跨 crate API 承诺。写下前问自己：如果签名要改，会不会犹豫？
- 不要为了测试把内部项设为 `pub`；用 `#[cfg(test)]` 模块访问 `pub(crate)` 项。

### 4.2 模块边界

- `aios-spec` 只放数据结构和协议 traits，零业务逻辑、零平台依赖。
- `aios-core` 是隐私与策略边界；`PrivacyAirGap`、`PolicyEngine`、
  `ActionLifecycle` 必须放在这里。
- `aios-action` 只执行 `AuthorizedAction`，不能自行 seal 授权动作。

---

## 5. 类型系统

### 5.1 enum >> bool

状态多于两种时立即用 `enum`。

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

语义不同的同类型用 newtype 区分。

```rust
#[derive(Debug, Clone, Copy)]
struct WindowOrdinal(u32);
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
}
```

### 6.2 避免 `unwrap()` / `expect()`

- 库代码避免 `unwrap()` / `expect()`；用 `Result`/`Option` 传播。
- 若确实逻辑不可达，用 `unreachable!()` 并加注释说明为什么外部输入触达不到。
- 测试代码除外。

---

## 7. 文档与注释

### 7.1 Doc comment

- 每个 `pub` / `pub(crate)` 项都必须有 doc comment。
- 首行是以句号结尾的单句摘要。
- 复杂不变量必须写明。

### 7.2 `unsafe`

每个 `unsafe` 块必须带 `// SAFETY:` 注释，说明：

1. 为什么必须用 `unsafe`。
2. 调用前置条件。
3. 为什么这些条件在本地可验证。

```rust
// SAFETY: `redirect_stdio_to_dev_null` 只在 daemonize 后的单进程上下文中调用，
// 此时 stdio 已关闭，打开的 `/dev/null` fd 是合法且唯一的。
let fd = unsafe { libc::dup(0) };
```

---

## 8. 测试

### 8.1 单元测试

- 放在被测试代码同文件的 `#[cfg(test)] mod tests { ... }` 中。
- 命名：`test_<function_name>__<scenario>`。

### 8.2 边界护栏

- 新增 `RawEvent` 必须补 `aios-spec` 类型、`PrivacyAirGap` 脱敏测试、窗口聚合测试。
- 新增动作必须补 `PolicyEngine` 审查测试和 `aios-action` 执行结果测试。
- 涉及 action-loop 的变更需通过 mock-socket 验证。

---

## 9. 依赖管理

- 新增依赖前检查：标准库能否替代？依赖健康度？传递依赖体积？
- 可执行程序提交 `Cargo.lock`；库 crate 在 `Cargo.toml` 中指定兼容版本范围。
- 默认关闭所有 optional feature，`default = []`。

---

## 10. 工作流

### 10.1 分支命名

```text
feat/<short-name>
fix/<short-name>
docs/<short-name>
```

### 10.2 Commit message

使用 Conventional Commits：

```text
feat(core): add capability check for local evaluator
fix(agent): route fallback when cloud misconfigured
docs(readme): clarify authorized action boundary
```

### 10.3 提交前检查

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --workspace --all-targets
```

---

## 11. 快速决策速查表

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

---

## 参考资料

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Book - Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/)
