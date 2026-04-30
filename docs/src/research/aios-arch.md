# AIOS 架构分析与供参考设计 (PDF)

本页面为 DiPECS 项目的核心架构参考文档。

---

## 摘要

1.  Agent 不进内核，Agent 成为系统级受控服务
2.  必须引入 System Action Fabric，统一动作协议
3.  模型只负责理解与规划，最终执行必须被策略、审计、确认机制约束

---

## 一、AIOS 的总目标

传统手机 OS 的核心对象是：
- App
- Window
- Process
- File
- Notification

模型原生 OS 的核心对象应变成：
- **Intent**（用户目标）
- **Action**（结构化系统动作）
- **Policy**（策略与授权）
- **Context**（环境与状态）
- **Agent**（持续执行主体）
- **Memory**（跨会话记忆）
- **Trust**（可审计、可追责、可确认）

**结论**：传统 OS 管“资源”；AIOS 既管资源，也管“任务闭环”。

---

## 二、设计原则

### 1) Intent-first，不是 screen-first
用户说“给张三发消息”“帮我订票”，系统应先理解为意图，再映射为结构化动作链，而不是直接让模型看屏幕找按钮。最新安全研究明确指出，“Screen-as-Interface”方案在高风险场景更脆弱；更安全的方向是 clean-slate secure agent OS + action access control + on-device audit。

### 2) Structured-action-first，GUI fallback
执行优先级应为：
1. 结构化系统动作
2. 系统服务 API
3. 应用能力接口
4. GUI Agent 兜底

这与最近的移动 Agent 研究一致：先走确定性控制路径，再升级到概率型 UI 推理，可显著减少不必要的模型调用、状态序列化和执行不确定性。

### 3) Agent 是系统一等公民，但不是内核本体
Agent Runtime 应是系统级服务，不是 kernel module。Android 官方架构本身就是 `framework → system services → HAL → native daemons/libraries → kernel`，系统能力通过 system services 暴露；Binder 则是标准 IPC。

### 4) 云 + 端协同是默认，不是补丁
- **端侧负责**：低延迟、隐私敏感、常驻感知、快速确认、小闭环任务。
- **云侧负责**：长上下文、重规划、多工具复杂推理、跨设备/跨应用长期任务。

### 5) 安全边界比“模型能力”更重要
近年的边缘 Agent 研究表明，部署架构本身就是主要风险来源：混合架构会引入边界穿越、故障窗口、可追溯链断裂等系统级风险。

---

## 三、完整分层架构

AIOS 建议分为 **8 层/模块**：

| 层级 | 模块名称 | 核心职责 |
| :--- | :--- | :--- |
| **L8** | Experience Layer | 统一入口与交互体验 |
| **L7** | Agent Runtime Module | Trusted Agent Runtime（系统级智能体） |
| **L6** | Intent & Memory Module | 意图理解、会话状态、长期记忆 |
| **L5** | System Action Fabric | 系统能力总线（**AIOS 关键新层**） |
| **L4** | System Service Module | 系统服务 / 权限 / 语义上下文 / 设备服务 |
| **L3** | Framework Layer | 开发框架 / SDK / App Ability 接口 |
| **L2** | Runtime & Native Layer | Binder、推理运行时、本地向量库 |
| **L1** | Kernel / HAL Layer | 调度、内存、驱动、进程隔离、安全边界 |

---

## 四、各层详细设计

### L1: Kernel / HAL Layer
最底层，只做确定性、可验证、低语义的事：
- **责任**：调度、内存管理、驱动、进程隔离、IPC 驱动、安全执行边界、硬件抽象。
- **为何不能放 Agent**：内核适合做“谁能运行、访问什么、消息传输、资源分配”，不适合做任务规划、模糊推理、长时状态机、第三方工具编排、语义决策。

### L2: Runtime & Native Layer
AIOS 的“操作底盘”，包含：
- Binder / IPC / RPC
- Native daemons
- Inference runtime（NN / GPU / NPU / DSP 调度）
- Secure storage / key service
- Logging / tracing / observability
- Local vector store / retrieval runtime

### L3: Framework Layer
统一框架层，对开发者暴露三类接口：
1.  **App API**：传统 UI / 数据 / 媒体 / 网络。
2.  **Ability API**：把 App 的高价值能力结构化暴露给系统。
3.  **Agent API**：给系统级 Agent 使用的受控接口。

**新要求**：每个 App 不仅要有 GUI，还需声明可供 AI 调用的 Abilities（输入输出 schema、风险级别、是否可后台/撤销）。

### L4: System Service Layer
强化传统 OS 服务，并新增 AIOS 专属服务：
- **核心系统服务**：Identity、Permission、Notification、App Lifecycle、Payment Bridge、Accessibility、Sensor、Search、Privacy Redaction。
- **AIOS 新增服务**：
    - **Semantic Context Service**：提供结构化上下文，非纯截图。
    - **Safety Interceptor Service**：高风险动作统一拦截点。

### L5: System Action Fabric（核心创新层）
将系统能力从“零散 API / GUI 操作”升级为统一的结构化动作总线。

- **Action 统一模型**：每个动作需包含 `type`、`target`、`params`、`permission_requirements`、`risk_level`、`rollback_semantics`、`audit_requirements`、`human_confirmation_policy`。
- **典型动作类型**：`APP.OPEN`、`MESSAGE.SEND`、`CALL.START`、`FILE.READ`、`PAYMENT.PREPARE` 等。
- **关键机制**：
    1.  **权限要求**：动作级权限声明（如 `MESSAGE.SEND` 需读取联系人权限，而非全局授权）。
    2.  **可回滚性**：Hard rollback（真撤销）、Compensating rollback（补偿）、No rollback。
    3.  **审计日志**：链路 ID、主体、权限、结果、脱敏。
    4.  **人机确认点**：自动执行、轻确认、强确认（生物识别）、永不自动执行。

### L6: Intent & Memory Layer
负责理解任务但不直接执行。
- **子模块**：Intent Parser、Task Graph Builder、Context Fusion、Short/Long-term Memory、Preference Model、Retrieval Engine、Uncertainty Estimator。
- **Memory 分层**：Session memory、Personal preference memory、Task template memory、Enterprise policy memory。
- **Taint-aware memory**：区分可信与不可信内容，防止污染长期偏好。

### L7: Trusted Agent Runtime
系统级、沙盒化、受控的 Agent 运行时。
- **8 个核心子模块**：
    1.  **Gateway**：统一入口（语音、通知、车机、云）。
    2.  **Planner**：拆解目标为多步计划（含回退路径）。
    3.  **Tool Router**：选择执行路径（Fabric -> API -> App Ability -> GUI fallback）。
    4.  **Policy Engine**：风险把关、域策略、用户偏好决策。
    5.  **Audit Engine**：全链路留痕、溯源、异常检测。
    6.  **Human Confirmation**：人机共驾确认点。
    7.  **Multi-Agent Coordinator**：拆分任务给不同领域 Agent。
    8.  **Safety Sandbox**：隔离第三方 Skills/Tools。

### L8: Experience Layer
全系统可唤起的自然交互界面：
- **统一入口**：语音唤醒、Spotlight 搜索、锁屏摘要、通知建议、浮动助理、App 内 agent sheet、摄像头截图叠加。

---

## 五、控制平面与数据平面

AIOS 必须明确拆分为两个平面：

| 平面 | 职责 | 包含内容 |
| :--- | :--- | :--- |
| **Control Plane** | 决定“做什么、能不能做” | Intent parsing、Planning、Policy、Routing、Scheduling、Confirmation |
| **Data Plane** | 负责“执行与数据流动” | IPC/RPC、App ability call、File I/O、Sensor stream、Media pipeline、Cloud tool invocation |

---

## 六、模型层设计

### 1) 三层模型拓扑
- **A. Nano/Micro Model（端上常驻）**：意图分类、唤醒、轻量总结、快速路由、本地隐私判定。
- **B. Local Mid-size Model（端上主力）**：多步轻任务、通知理解、UI 语义理解、短规划、本地工具调用。
- **C. Cloud Frontier Model（云上重推理）**：长链推理、跨应用编排、文档处理、多代理协调、企业策略融合。

### 2) 路由维度
模型路由至少考虑：latency、cost、battery、privacy、risk、offline availability、context length。

### 3) 模型权限边界
模型只提出 **Plan**、**Parameter proposal**、**Confidence**、**Risk estimate**，最终执行权由 **Policy Engine + System Action Fabric + Human Confirmation** 共同决定。

---

## 七、记忆系统设计

记忆不能等同于“无限收集聊天记录”。

- **Ephemeral Memory**：临时会话上下文，任务结束即衰减。
- **Preference Memory**：用户长期偏好（常用联系人、通知优先级）。
- **Procedural Memory**：可复用任务模板（如“下班前发日报”）。
- **Restricted Memory**：高敏感信息（支付/健康/凭据），单独加密与审批。

**关键原则**：最小必要持久化、来源标签、可删除、可解释、外部不可信内容不得自动升格为长期偏好。

---

## 八、安全架构

AIOS 的成败本质上是安全架构问题。

### 1) 四层安全环
1.  **Identity**：Agent identity、tool identity、app identity、session binding。
2.  **Capability**：最小权限、短时令牌、域级授权、一次性授权。
3.  **Interception**：高风险动作统一拦截，支付/删除/外呼必须经过关键节点。
4.  **Accountability**：本地不可抵赖审计、provenance chain、replayable logs。

### 2) 混合架构风险
云+端需明确主权边界，防止 fallback 静默越界和溯源绕过。
- **对策**：云调用显式标注、fallback 可见、离线降级策略、跨边界动作留痕。

---

## 九、应用生态如何重构

AIOS 不应消灭 App，而应重构 App 为三层结构：
1.  **UI Surface**：给人看的界面。
2.  **Ability Surface**：给系统/Agent 调用的结构化能力。
3.  **Policy Surface**：声明能力的自动/确认/禁止执行策略。

**新的 App Store 审核维度**：是否提供能力 schema、是否幂等、是否可回滚、风险级别、audit hooks 支持、无 GUI 执行能力。

---

## 十、Android 与 OpenHarmony 的落地映射

### Android 路线
- **基础优势**：modular system services、Binder IPC、AIDL、HAL。
- **推荐落地组件**：
    - `agentd`：Trusted Agent Runtime
    - `actiond`：System Action Fabric executor
    - `policyd`：策略服务
    - `auditd`：审计服务
    - `confirmd`：确认 UI / 生物识别桥
    - `semantid`：UI 语义树和上下文服务

### OpenHarmony 路线
- **基础优势**：分层架构（Kernel -> System Service -> Framework）、System Abilities (SA) 机制、独立沙盒运行。
- **推荐落地**：TAR 作为新的 System Ability 集群，通过 SA Manager 暴露 Agent / Action / Policy 能力，利用 permission framework + sandbox 隔离 skills。

---

## 十一、AIOS 的标准执行流

以 **“给张三发消息：我10分钟后到”** 为例：
1.  入口层接收语音/文字。
2.  Intent 层识别为 `Communicate -> MessageSend`。
3.  Memory 层解析“张三”为常用联系人。
4.  Planner 生成计划。
5.  Tool Router 优先选择 `MESSAGE.SEND` 结构化动作。
6.  Policy Engine 判断是否需确认。
7.  Human Confirmation 弹出确认。
8.  System Action Fabric 执行动作。
9.  Audit 记录链路。
10. Experience Layer 返回结果与后续建议。

**注**：模型参与规划，**不直接裸执行**。

---

## 十二、与“AI 助手 + 普通 OS”的根本区别

| 普通做法 | AIOS 做法 |
| :--- | :--- |
| 一个 App 内的助手 | 系统级 Agent Runtime |
| 大量依赖截图 + 点击 | 结构化 Action 总线 |
| 权限碎片化 | 统一策略与审计 |
| 难审计难复现 | 人机共驾内建 |
| 遇到 UI 变化就脆弱 | 云/端模型协同，App 能力协议化 |

**本质**：AIOS 不是“更强的自动化”，而是 **“把自动化提升为操作系统级的受控执行语义”**。

---

## 十三、未来 3 个阶段的演进路线

1.  **阶段 1：Agent-augmented OS**
    - 仍是传统 OS，加入系统级 Agent。
    - 先做 Action Fabric 和审计。

2.  **阶段 2：Intent-native OS**
    - App 不再是唯一入口，任务可跨 App 原生执行。
    - 用户更多以目标而非 App 名称交互。

3.  **阶段 3：Model-native OS**
    - 调度器同时调度 Compute + Models + Agents。
    - 多设备共享 Context / Memory / Action Protocol。
    - 交互演进为：“发目标 + 看中间态 + 接管关键节点”。

---
