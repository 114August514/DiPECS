# Lab4 部署说明

## 总览

本文记录 `llama.cpp` 单机部署、RPC 分布式部署和 Ceph 扩展环境。每条命令都应保留运行机器、工作目录和输出路径，便于复现实验。`lab4-llama` 只用于 Rust smoke，不作为正式推理结果。

## 分项一：硬件与系统环境

环境采集命令：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-env
```

| 节点 | 角色 | CPU | 内存 | GPU | OS / Kernel | IP | 备注 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| host-a | 主机 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 运行 `llama-cli` |
| host-b | 从机 | 待填写 | 待填写 | 待填写 | 待填写 | 待填写 | 运行 `rpc-server` |

## 分项二：模型信息

| 字段 | 内容 |
| :--- | :--- |
| 模型名称 | 待填写 |
| 参数规模 | 待填写 |
| GGUF 量化格式 | 待填写 |
| 文件大小 | 待填写 |
| 来源 | 待填写 |
| 本地路径 | 待填写 |

## 分项三：llama.cpp 单机部署

拉取第三方依赖：

```bash
git submodule update --init --recursive
cd lab4/third_party/llama.cpp
git rev-parse HEAD
# c4a278d68efa17811006f2123a84081dac03fac7
cmake -B build -S . -DCMAKE_BUILD_TYPE=Release -DGGML_RPC=ON
cmake --build build --config Release -j "$(nproc)"
```

单机推理命令：

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -p "用中文解释什么是操作系统页缓存。" \
  -n 128 \
  --threads 8
```

Rust 工具批量测量命令：

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

## 分项四：RPC 分布式部署

从机命令：

```bash
lab4/third_party/llama.cpp/build/bin/rpc-server -H 0.0.0.0 -p 50052
```

主机推理命令：

```bash
lab4/third_party/llama.cpp/build/bin/llama-cli \
  -m lab4/data/models/qwen2.5-1.5b-instruct-q4_k_m.gguf \
  -p "RPC 分布式推理会引入哪些网络开销？" \
  -n 128 \
  --threads 8 \
  --rpc 192.168.1.10:50052
```

## 分项五：Ceph 环境

| 字段 | 内容 |
| :--- | :--- |
| 部署方式 | 待填写 |
| Monitor 数量 | 待填写 |
| OSD 数量 | 待填写 |
| Pool / CephFS | 待填写 |
| 副本数 | 待填写 |
| 挂载或对象路径 | 待填写 |

存储测量命令示例：

```bash
cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-storage -- read \
  --case-id local-model-read-001 \
  /path/to/model.gguf \
  --output lab4/data/results/storage-local-read.jsonl

cargo --manifest-path lab4/Cargo.toml run -p lab4-tools --bin lab4-storage -- read \
  --case-id ceph-model-read-001 \
  /mnt/ceph/model.gguf \
  --output lab4/data/results/storage-ceph-read.jsonl
```
