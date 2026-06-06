# llama.cpp 与 GGUF 模型接入

## 结论

正式 Lab4 实验使用 `llama.cpp` 完成真实模型推理、参数优化和 RPC 分布式推理；本仓库的 Rust 代码负责 prompt 校验、命令封装、JSONL 记录、结果汇总和 Ceph 存储测量。

不要把 `llama.cpp` 源码复制进仓库，也不要把 GGUF 大模型提交进 Git。

## 目录约定

```text
lab4/
├── third_party/
│   └── llama.cpp/          # git submodule，只提交指针
├── data/
│   ├── models/             # 本地 GGUF 模型，gitignore
│   ├── prompts/
│   ├── raw/
│   └── results/
└── docs/
```

## 拉取 llama.cpp

推荐使用 submodule：

```bash
git submodule add https://github.com/ggml-org/llama.cpp lab4/third_party/llama.cpp
git submodule update --init --recursive
```

如果已经存在 submodule 记录，其他机器只需要：

```bash
git submodule update --init --recursive
```

固定版本：

```bash
cd lab4/third_party/llama.cpp
git rev-parse HEAD
```

把 commit hash 写入 `lab4/reports/deployment.md`，避免报告只有“最新版”这种不可复现描述。

## 构建 llama.cpp

### 环境准备

需要 `cmake` 和 `clang`（或兼容的 gcc）。**推荐 Clang**：GCC 16+ 对当前 llama.cpp 某些版本存在兼容性警告（如方法调用语法检查更严格），用 Clang 可直接避免。

Ubuntu/Debian 示例：

```bash
sudo apt install cmake clang
```

### CPU + RPC 构建

```bash
cd lab4/third_party/llama.cpp
rm -rf build

CC=clang CXX=clang++ cmake -B build -S . -DCMAKE_BUILD_TYPE=Release -DGGML_RPC=ON
CC=clang CXX=clang++ cmake --build build --config Release -j "$(nproc)"
```

验证可执行文件：

```bash
./build/bin/llama-cli --version
./build/bin/llama-bench --help
./build/bin/rpc-server --help
```

如果机器有 NVIDIA GPU，可另建一个 CUDA build 目录：

```bash
cd lab4/third_party/llama.cpp
cmake -B build-cuda -S . -DCMAKE_BUILD_TYPE=Release -DGGML_CUDA=ON -DGGML_RPC=ON
cmake --build build-cuda --config Release -j "$(nproc)"
```

不要创建 Makefile。课程实验命令和本仓库自动化入口统一使用 Cargo、CMake 和文档命令。

## 下载 GGUF 模型

推荐先用 Qwen2.5 1.5B Instruct 的 Q4_K_M 量化模型，体积和质量比较适合普通笔记本。

下载模型（推荐用镜像站，国内访问更快）：

```bash
mkdir -p lab4/data/models

wget --show-progress -c \
  "https://hf-mirror.com/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf" \
  -O lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf
```

也可以让新版 `llama.cpp` 直接通过 `-hf` 拉取模型缓存：

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -hf Qwen/Qwen2.5-1.5B-Instruct-GGUF:Q4_K_M \
  -p "用中文解释什么是操作系统页缓存。" \
  -n 128
```

为了实验可复现，更推荐显式下载 `.gguf` 到 `lab4/data/models/`，然后用 `-m` 指定路径。

## 单机 smoke 测试

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -p "用中文解释什么是操作系统页缓存。" \
  -n 128 \
  --threads 8
```

使用 Rust 工具记录结果：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-bench -- \
  --prompts lab4/data/prompts/quality-prompts.jsonl \
  --executable lab4/third_party/llama.cpp/build/bin/llama-cli \
  --model lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  --output lab4/data/results/single-quality.jsonl \
  --mode single \
  --case-prefix single-quality \
  --arg=--threads --arg=8
```

## RPC smoke 测试

从机：

```bash
lab4/third_party/llama.cpp/build/bin/rpc-server -H 0.0.0.0 -p 50052
```

主机：

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -p "RPC 分布式推理为什么可能比单机更慢？" \
  -n 128 \
  --threads 8 \
  --rpc 192.168.1.10:50052
```

`rpc-server` 不要暴露到公网。只在可信局域网或实验网络中运行。

## 记录要求

每次正式实验至少记录：

- `llama.cpp` submodule commit hash。
- 构建参数，如 `-DGGML_RPC=ON`、`-DGGML_CUDA=ON`。
- 模型 repo、文件名、量化格式、文件大小。
- 单机命令、RPC 主从机命令。
- JSONL 结果路径。
- 机器硬件、OS、内核和网络环境。

## 参考来源

- `llama.cpp` 官方构建文档：<https://github.com/ggml-org/llama.cpp/blob/master/docs/build.md>
- `llama.cpp` RPC 文档：<https://github.com/ggml-org/llama.cpp/blob/master/tools/rpc/README.md>
- Qwen2.5 1.5B Instruct GGUF：<https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF>
