# Rust 实现约束

## 总览

Lab4 后续代码统一使用 Rust 编写，不引入 Makefile。自动化流程通过 Cargo 二进制、测试和文档命令完成。代码风格遵守仓库中的 [Rust 编码规范](../../docs/src/team/conventions/rust.md)。

## 分项一：目录规划

当前在 `lab4/` 下使用如下结构：

```text
lab4/
├── docs/
│   ├── README.md
│   ├── task-breakdown.md
│   ├── os-knowledge.md
│   └── rust-implementation.md
├── data/
│   ├── prompts/
│   ├── results/
│   └── raw/
├── crates/
│   └── lab4-tools/
└── reports/
```

其中：

- `data/prompts/` 保存 prompt 数据集。
- `data/results/` 保存整理后的 JSONL 或 CSV 结果。
- `data/raw/` 保存原始命令输出、截图索引或日志。
- `crates/lab4-tools/` 保存 Rust 测量工具。
- `reports/` 保存最终报告草稿和表格。

## 分项二：工具边界

Rust 工具优先承担以下任务：

- 调用 `llama-cli`、`llama-bench` 或 `llama-server`。
- 记录启动时间、结束时间和退出状态。
- 解析关键性能输出，例如 tokens/s、耗时、生成 token 数。
- 读取 prompt 数据集，批量执行测试。
- 输出 JSONL 结果，保留每条请求的原始参数和摘要指标。
- 对比本地路径与 Ceph 路径的读写耗时。

Rust 工具不负责：

- 重新实现 LLM 推理。
- 替代 `llama.cpp` 的 RPC backend。
- 用隐式脚本修改系统级 Ceph 配置。

## 分项三：编码规范要点

后续写代码时执行以下规则：

- 文件名、模块名、函数名使用 `snake_case`。
- 类型、枚举和 trait 使用 `PascalCase`。
- 函数可能失败时返回 `Result<T, E>`，不使用 `unwrap()`。
- 应用层可使用 `anyhow::Context` 添加错误上下文。
- 库层错误使用 `thiserror` 定义结构化错误。
- 参数类型优先使用 `&str`、`&Path` 等借用形式。
- 不为绕过 borrow checker 随意 `clone()`。
- 每个 `.rs` 文件顶部写 `//!` 模块级文档。
- 每个 `pub` 或 `pub(crate)` 项写 `///` 文档注释。
- `unsafe` 默认不用；确需使用时写 `// SAFETY:` 并保持块最小。

## 分项四：命令入口

不创建 Makefile。常用入口建议写入文档或 Cargo 子命令：

```bash
cargo fmt --manifest-path lab4/Cargo.toml
cargo check --manifest-path lab4/Cargo.toml
cargo clippy --manifest-path lab4/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path lab4/Cargo.toml --all-targets
```

当前 `lab4-tools` crate 提供如下二进制：

- `lab4-llama`：Rust smoke 入口，包含 `infer`、`bench`、`rpc-worker` 和 `rpc-master`，只用于验证本仓库流程，不替代正式 `llama.cpp` 推理。
- `lab4-bench`：运行单机或 RPC 推理测量。
- `lab4-prompts`：校验 prompt 数据集格式。
- `lab4-storage`：测量本地路径与 Ceph 路径读写耗时。
- `lab4-summarize`：读取 JSONL 结果并输出汇总表。
- `lab4-env`：采集当前机器的 OS、内核、CPU 和内存信息。

示例：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-prompts -- lab4/data/prompts/quality-prompts.jsonl

cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-env

# smoke：验证 Rust 加载、JSONL 和 RPC 流程，不作为正式 LLM 结果
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-llama -- infer \
  --model lab4/data/models/placeholder.model \
  --prompt "从操作系统角度解释 mmap 和页缓存" \
  --max-tokens 80 \
  --threads 4 \
  --batch-size 8 \
  --ctx-size 1024

cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-llama -- bench \
  --prompts lab4/data/prompts/quality-prompts.jsonl \
  --model lab4/data/models/placeholder.model \
  --output lab4/data/results/smoke-rust-llama-quality.jsonl \
  --mode single \
  --case-prefix smoke-rust-llama-quality \
  --threads 4 \
  --batch-size 8 \
  --ctx-size 1024

# 正式实验：调用 llama.cpp
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-bench -- \
  --prompts lab4/data/prompts/quality-prompts.jsonl \
  --executable lab4/third_party/llama.cpp/build/bin/llama-cli \
  --model lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  --output lab4/data/results/single-quality.jsonl \
  --mode single \
  --case-prefix single-quality \
  --arg=--threads --arg=8

cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-storage -- read \
  --case-id local-model-read-001 \
  /path/to/model.gguf \
  --output lab4/data/results/storage-local-read.jsonl
```

## 分项五：数据格式

建议 prompt 数据使用 JSONL，每行一个对象：

```json
{"id":"os-001","category":"os","prompt":"解释 mmap 在模型加载中的作用。","max_tokens":128}
```

建议测量结果也使用 JSONL：

```json
{"case_id":"single-baseline-001","prompt_id":"os-001","mode":"single","started_at":"2026-06-05T22:00:00+08:00","duration_ms":12345,"exit_code":0,"tokens_per_second":18.2}
```

字段应稳定，缺失指标使用 `null` 或省略，但不要在同一文件中混用多种含义。

## 分项六：测试与验证

Rust 工具至少需要覆盖：

- prompt JSONL 解析。
- 命令配置生成。
- 输出指标解析。
- 统计汇总。
- 存储路径计时结果的单位换算。

测试命名遵守 `test_<function_name>__<scenario>`。涉及外部命令或 Ceph 环境的测试应标注为集成测试，并允许通过环境变量显式启用，避免普通 `cargo test` 被本机环境卡住。

## 分项七：报告复现要求

每个实验命令旁边应记录：

- 运行机器。
- 工作目录。
- 完整命令。
- 模型路径和模型 hash，若无法记录 hash，则记录文件大小和来源。
- prompt 数据版本。
- 输出结果路径。
- 失败时的 stderr 路径。

这样最终报告可以从 `data/results/` 追溯到原始命令和系统环境。
