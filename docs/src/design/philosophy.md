# 设计哲学

## OS = 对象 + API

DiPECS 重新定义了操作系统的基本对象：

| 传统 OS | DiPECS |
|:--------|:-------|
| `Process`, `File`, `Socket` | `Intent` (意图), `Action` (动作), `Context` (上下文), `Policy` (策略) |

API 就是这些对象之间的转换函数：

```text
solve(Intent, Context)   → Plan<Actions>
verify(Action, Policy)   → AuthorizedAction | Denied
```

## 五大核心模块

### 1. `aios-spec` — 宪法层

整个项目的 **Single Source of Truth**。定义核心数据结构、Trait 接口和 Function Calling Schema。

工程意义：只要 `spec` 不动，各组可以并行开发。协议变更必须走 RFC 流程。云端返回的 JSON 必须完美契合这里的 Rust `struct`——模型升级导致格式变化时，spec 必须能兼容。

### 2. `aios-kernel` — 触手层

AIOS 与底层 Android/Linux 的通信边界：

- **拦截器**：通过 eBPF 或 LSM 截获 App 的系统调用
- **注入器**：把 Agent 生成的动作（点击、发包）转换成 Linux 指令

运行在特权级最高的区域，要求高性能、低侵入。

### 3. `aios-core` — 脊梁层

这是系统的 **Action Bus（动作总线）**，核心职责：

- **调度**：决定哪个 Action 先执行
- **策略引擎 (Policy Engine)**：内核级"防火墙"，根据 Policy 判定 AI 产生的动作是否安全（例如深夜不能自动支付、转账必须人工确认）
- **Privacy Filter (隐私滤镜)**：数据出海前进行正则或轻量语义脱敏——这是 DiPECS 最核心的模块之一
- **Action Verifier**：云端 LLM 可能产生错误指令，执行前必须做 100% 静态类型检查和安全过滤
- **Trace Engine**：全链路确定性记录，支持 Golden Trace 回归验证

实现方式：Rust 同步优先（不引入不必要的 async），异步点集中在系统边界。

### 4. `aios-agent` — 大脑层

云端驱动的架构下，agent 不再是本地推理机，而是 **会话管理器**：

- **Planner**：将模糊意图拆解为具体 Action 链
- **Memory**：管理长期偏好和短期会话状态
- **CloudProxy**：API 调用优化——连接池管理、Token 消耗统计、多轮对话状态缓存
- **降级策略**：云端超时或不可用时，使用本地保守策略 fallback

### 5. `aios-adapter` — 适配层

虚拟化层，通过 Rust Trait 实现 **Offline/Online 零成本切换**：

- **Offline**：读取 `data/traces/` 的 Golden Trace 进行确定性回放验证
- **Online**：调用 `aios-kernel` 的 Binder 接口，或通过 HTTPS/gRPC 与云端通信

## 云端驱动的数据流

大脑在云端，本地 OS 的本质是一个 **"带隔离功能的语义执行器"**。

```text
采集 (Kotlin) → 序列化 → 脱敏 (PrivacyAirGap) → 窗口聚合 → 云端 LLM
                                                           ↓
优化执行 (Kotlin) ← 策略校验 (PolicyEngine) ← 结构化输出 ←┘
     ↓
Trace 记录 → Golden Trace 回归验证
```

六个环节：

1. **Perception** — 本地 OS 抓取 UI 树、传感器数据、用户上下文
2. **Redaction** — 数据出海前自动识别并抹除 PII（姓名、卡号等），替换为占位符
3. **Request** — 将结构化 Context + Intent 发送给云端 API
4. **Reasoning** — 云端返回结构化的 Action List（JSON/Protobuf）
5. **Execution** — 本地 Action Bus 解析 JSON，转化为真正的系统调用
6. **Observation** — 执行结果反馈到云端，决定下一步

系统要解决的最核心问题不是"模型准不准"，而是**语义鸿沟**：云端说"把这个文件发给张三"，本地 OS 必须精准定位——哪个文件？哪个张三？对应的 fd 是什么？App 权限够不够？

> Action Bus 是 AI 时代的系统调用接口。传统 `syscall` 传的是寄存器数值，AI-syscall 传的是语义对象。
>
> — JYY

## 一个意图的生命周期

以"给张三发 50 块红包"走一遍完整流程：

1. **输入**：Experience Layer 捕获语音，产生原始 `Intent`
2. **解析** (`aios-agent`)：模型通过 Memory 找到张三的 ID，制定计划 `[Search(张三), Pay(50)]`
3. **分发** (`aios-core`)：动作进入 Action Bus
4. **审计** (Policy Engine)：查询策略，发现"支付额度 > 20 需要人工确认"
5. **交互**：弹出确认框给用户
6. **执行** (`aios-kernel`)：用户确认后，通过 adapter 调用支付
7. **观测**：管理员看到支付 Action 生命周期结束，状态变为 `COMPLETED`

## 工程防线

- **`data/traces`** — 离线轨迹数据是算法组的"粮草"。开发时大量依赖离线 Trace 回放测试 Action Bus 逻辑，而非每次都调云端 API
- **`tools/aios-replay`** — 调试组的"时光机"。系统崩溃时一帧帧重放失败过程，定位错误 Action
- **`docs/rfc`** — 架构组的"刹车闸"。防止接口每天变化导致项目无法编译
- **`scripts/setup-env.sh`** — 新人的"入职礼"。10 分钟内跑通 Hello World
