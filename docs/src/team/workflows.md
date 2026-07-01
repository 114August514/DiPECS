# 开发工作流

> Status: Current  
> Last verified: 2026-07-01  
> Code anchors: `CONTRIBUTING.md`, `.github/workflows/`, `scripts/`

**这篇文档回答什么**：如何以最小摩擦向 DiPECS 提交代码、通过 CI、并把文档同步更新。  
**适合谁读**：第一次给 DiPECS 贡献代码或文档的开发者。

## TL;DR

1. 从 `main` 切出 `feat/` / `fix/` / `docs/` 分支。
2. 写代码 + 测试 + 文档；本地跑 `cargo fmt`、`cargo clippy`、`cargo test`。
3. 提交符合 Conventional Commits 的 commit message。
4. 开 PR，描述清楚问题、改动、验证方式。
5. CI 全绿 + 至少一位模块维护者 review 后，squash merge。

## 分支命名

| 类型 | 前缀 | 示例 |
| --- | --- | --- |
| 新功能 | `feat/` | `feat/cloud-llm-provider` |
| Bug 修复 | `fix/` | `fix/proc-reader-diff` |
| 文档 | `docs/` | `docs/decision-routing` |

不要在同一分支里混功能改动和大量格式整理；格式整理单独一个 PR。

## Commit message

使用 Conventional Commits：

```text
<type>(<scope>): <subject>
```

常用 type：

- `feat` — 新功能
- `fix` — 修复
- `docs` — 文档
- `test` — 测试
- `refactor` — 重构
- `perf` — 性能
- `ci` — CI 配置
- `chore` — 杂项

示例：

```text
feat(spec): add collector envelope for app transition
fix(core): reject over-capability actions in local evaluator
docs(readme): clarify authorized action boundary
test(action): add android bridge envelope coverage
```

仓库配置了 `scripts/commit-msg.sh` hook，提交时会校验格式。如需绕过可用 `git commit --no-verify`，但不推荐。

## 本地检查清单

每次提交前建议运行：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

如果需要完整验证（含 Android 交叉编译）：

```bash
source scripts/setup-env.sh
./scripts/check-all.sh
```

`scripts/check-all.sh` 会跑：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check --workspace --target aarch64-linux-android --all-targets`

## 运行项目

### Daemon

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- --no-daemon
```

带 Android JSONL：

```bash
RUST_LOG=info cargo run -p aios-daemon --bin dipecsd -- \
  --no-daemon \
  --android-trace-jsonl apps/android-collector/actions.jsonl \
  --trace-output data/evaluation/runtime.ndjson
```

### CLI replay

```bash
cargo run -p aios-cli -- replay data/traces/sample_replay.jsonl \
  --stages policy \
  --audit data/evaluation/audit.ndjson
```

### Android collector

```bash
cd apps/android-collector
./gradlew :app:assembleDebug
```

## 文档更新

新增或修改 `docs/src/` 页面后：

1. 在 `docs/mkdocs.yml` 的 `nav:` 里加入入口。
2. 本地构建：

   ```bash
   cd docs
   uv sync
   PYTHONPATH=. uv run mkdocs build
   ```

3. 确保没有 broken internal link 警告。
4. 如果新增了 cross-reference，更新相关文档的“相关文档”列表。

## 提交 PR

使用 `.github/pull_request_template.md`，包含：

- 改动类型（feat/fix/docs/...）
- 问题背景
- 具体改动
- 验证方式（运行了哪些命令）
- 关联的 RFC / Issue（架构或协议变更必须关联）

要求：

- 改动聚焦，不要把功能改动和格式整理混在一起。
- 至少一位相关模块维护者 review。
- 跨模块协议变更需要更严格 review。
- CI 红了不能合并。

## CI 检查说明

| Workflow | 触发条件 | 检查内容 |
| --- | --- | --- |
| `lint.yml` | Rust/Cargo 改动 | `cargo fmt`、`cargo clippy`、`cargo machete` |
| `test.yml` | Rust/Cargo 改动 | `cargo nextest`、`cargo test --doc`、replay fixture |
| `build.yml` | Rust/Cargo 改动 | Linux/Android release build、ELF audit |
| `audit.yml` | 定时/手动 | `cargo-deny`、依赖拓扑检查、CVE 扫描 |
| `docs.yml` | docs/ 改动 | `mkdocs build`、`cargo doc`、LaTeX 报告 |
| `android-collector.yml` | app/ 改动 | Android 单元测试、assembleDebug |
| `mirror_sync.yml` | 每次 push | 镜像到 `OSH-2026/DiPECS.git` |

## 合并方式

使用 **Squash and merge**：

- PR 分支会被删除。
- 合并前把 `Co-authored-by:` 行手动复制到 squash message。
- 合并后本地运行：

  ```bash
  git fetch origin main
  git switch main
  git pull
  ```

## 常见失误

| 问题 | 避免方法 |
| --- | --- |
| CI 上 clippy 失败 | 本地先跑 `cargo clippy --workspace --all-targets -- -D warnings` |
| 文档页面没出现在导航 | 记得改 `docs/mkdocs.yml` |
| commit message 被 hook 拒绝 | 按 `type(scope): subject` 格式写，长度 ≥ 10 |
| 合并后本地分支落后 | `git pull` 或 `git reset --hard origin/main` |

## 相关文档

- [开发指南](../team/dev.md)
- [环境配置](../team/environment.md)
- [CI 质检体系](../team/ci.md)
- [Rust 编码规范](../team/conventions/rust.md)
- [CONTRIBUTING.md](https://github.com/114August514/DiPECS/blob/main/CONTRIBUTING.md)
