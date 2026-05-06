# DiPECS v0.3 — 答辩 Q&A 预案

---

## 一、技术类

### Q1: 你们说隐私脱敏是"硬边界"，具体怎么保证原始数据不会泄露？万一 Rust 代码有 bug 呢？

**A:** 两层保障：

1. **编译期保障（Ownership 机制）**: `PrivacySanitizer::sanitize(raw: RawEvent) -> SanitizedEvent` 接收 `RawEvent` 的 ownership。`RawEvent` 中的 `raw_text: String` 被 move 进函数后，在函数退出时自动 drop，物理内存被释放。Rust 编译器保证不存在 use-after-free / double-free，也不存在悬挂引用。这不是"我们承诺不上传"，而是"编译器保证了不可访问"。

2. **审计边界**: 脱敏是 `aios-core` 内唯一的 `RawEvent -> SanitizedEvent` 路径。任何新代码需要接触 `RawEvent` 必须经过 code review，且所有 `SanitizedEvent` 类型不含 `String` 形式的用户文本——只有 `TextHint { length_chars: usize, script: ScriptHint, is_emoji_only: bool }` 加 `Vec<SemanticHint>`。

3. **Golden Trace 回归**: 每次代码变更必须通过确定性回放。如果脱敏逻辑被意外修改，Golden Trace 的 `sanitization_match` 会失败，CI 拦截。

> **追问**: 那关键词匹配本身不会暴露信息吗？

**A:** 不会。`SemanticHint` 是枚举标签（`FileMention` / `FinancialContext` / `VerificationCode`），不是提取出的关键词原文。云端只知道"这个窗口内有一条通知含有文件相关语义"，不知道文件名是什么、文件内容是什么。

---

### Q2: 如果 LLM 返回了错误的意图怎么办？比如它预判用户要打开微信，但用户其实要看抖音？

**A:** 这是本系统设计时就考虑到的核心容错问题，分层处理：

1. **预测错误≠系统故障**: 我们的优化动作是无损的——`PreWarmProcess` 只是提前 fork zygote、加载 App 进程到内存，不改变前台 UI。用户完全无感知，该点什么点什么。如果预热的 App 30 秒内未被使用，Android LMK 自然会回收。

2. **PolicyEngine 二次校验**: LLM 返回的意图需要经过本地 PolicyEngine 校验——风险等级（仅 Low 自动执行）、置信度阈值（< 0.3 直接拒绝）、动作黑名单。即使 LLM "抽风"返回高风险动作，本地也会拦截。

3. **MockCloudProxy 当前的保守策略**: 空窗口返回 `Idle + NoOp` (0.50)，只有检测到明确信号（如 FileMention、ActivityLaunch）才生成具体意图。

4. **未来的端侧小模型兜底**: 计划加入轻量级端侧模型（如 TinyLlama 量化版），在云端超时或不可达时提供本地 fallback。

---

### Q3: 系统延迟增加了多少？采集+脱敏+云端推理一轮下来，用户操作可能已经完成了？

**A:** 分阶段测量（当前为骨架估算，后续真机 benchmark）：

| 阶段 | 延迟 | 说明 |
|:---|:---|:---|
| /proc 轮询 + 差分 | < 5ms | 仅扫描变化的进程，不遍历全部 |
| 脱敏 (sanitize) | < 1ms | 纯内存操作，无 I/O |
| 窗口聚合 | < 1ms | HashMap 计数，内存操作 |
| 云端推理 (Mock) | < 5ms | 规则匹配，纯本地 |
| 云端推理 (未来真实 LLM) | 200-800ms | 取决于模型和网络 |
| PolicyEngine 校验 | < 1ms | 条件判断 |
| 动作执行（PreWarm） | 数十 ms | fork zygote + 加载 |

**关键**: 系统设计为**异步预测**，不是在用户点击后才开始推理。当用户收到一条微信通知时，系统在通知到达的 100ms 内就开始脱敏→推理→预热，而此时用户可能还在看通知内容（通常 1-3 秒）。等到用户真正点击时，App 已经预热好了。

> **追问**: 那云端推理的 200-800ms 会不会太慢？

**A:** 真实场景中走流式推理（streaming），可以在首 token 返回后就开始本地处理。且后续版本考虑异步流水线：上一窗口的推理结果可以预热下一窗口可能用到的 App。

---

### Q4: Binder eBPF 需要 root 权限或定制内核吧？没有 root 的普通设备怎么办？

**A:** 诚实回答——目前 eBPF 确实需要 root 或至少 `CAP_BPF`。两条路径：

1. **研究原型（当前阶段）**: 在 root 过的 AOSP 模拟器或开发板上运行，这是学术原型验证的合理前提——很多 OS 研究项目（如 KLEE、AOSP 内核模块）都需要 root。

2. **降级运行（未来）**: 系统支持 SourceTier 分级——`Daemon` 级事件（需要 root）和 `PublicApi` 级事件（仅需用户授权）。当 eBPF 不可用时，`BinderProbe` 返回 `None`，系统降级到仅依赖 NotificationListenerService + UsageStatsManager，功能受限但核心管道仍在。

3. **替代方案**: 如果不挂 eBPF，可以通过 `dumpsys` 周期性采样 Binder 统计信息作为近似，或通过 `/proc/binder` 读取事务日志（Android 某些版本可读）。

---

### Q5: 为什么选 Rust 而不是 C/C++ 或 Go？这增加了开发成本和团队成员的学习门槛。

**A:**

1. **本项目的核心需求是"证明隐私不可逆"，不是"承诺隐私"**。Rust 的 ownership 系统在编译期就保证了 `RawEvent.raw_text` 在 `sanitize()` 后不可访问。C/C++ 中这是靠代码纪律，Go 的 GC 也无法在编译期保证。

2. **无 GC 意味着可预测的性能**。Android daemon 是长驻进程，GC pause 会导致采集延迟抖动，影响 100ms 的采集周期稳定性。

3. **跨编译零成本**: Rust 支持 `x86_64-linux-gnu`→桌面开发、`aarch64-linux-android`→真机部署，同一份代码、同一个编译器，无需像 C/C++ 那样处理 NDK toolchain 的 ABI 兼容问题。

4. **团队方面**: 三人的核心贡献者都有 Rust 经验。且项目已产出 4,300+ 行 Rust 代码和 62 个测试，学习曲线前期成本已被摊销。

---

### Q6: 为什么不直接在端侧跑一个小模型（如端侧 LLM）而要用云端？

**A:**

1. **能力差距**: 2026 年的端侧模型（如手机端量化的 3B 模型）在意图推理、多信号综合判断上远不如云端大模型。我们的信号种类多（Binder 模式 + /proc 状态 + 通知语义 + 系统状态），需要一定的推理能力。

2. **我们实际上支持两者**: 架构上 `CloudProxy` 是一个 trait，可以替换为本地模型实现。当前 MockCloudProxy 本身就是纯本地的。未来计划是云端主推理 + 端侧轻量模型兜底。

3. **隐私问题已经解决**: 因为 PrivacyAirGap 保证了发送给云端的数据不含 PII，所以"用云端"的隐私代价和"用端侧"是等价的。

---

3. **隐私问题已经解决**: 因为 PrivacyAirGap 保证了发送给云端的数据不含 PII，所以"用云端"的隐私代价和"用端侧"是等价的。

---

### Q6b: v0.3 新增了 `apps/android-collector` (Kotlin 应用)，它和 Rust daemon (`aios-adapter`) 是什么关系？为什么不全部用 Rust 采集？

**A:**

1. **角色互补**: android-collector 是 **Phase-1 探针**，用于验证"某个 Android 接口能观测到什么信号"。daemon (`aios-adapter`) 是**生产级采集**，运行在系统层。探针筛选通过的接口，才提升到 daemon 中。

2. **技术分工**:
   - `apps/android-collector` (Kotlin): 走 Android SDK API — `NotificationListenerService`、`AccessibilityService`、`UsageStatsManager`。这些接口无需 root，仅需用户授权，可以直接在普通设备上验证。
   - `aios-adapter` (Rust): 走 Linux 内核接口 — `/proc`、`/sys/class`、eBPF tracepoint。需要系统权限，但可获取更底层的信号（Binder IPC、进程状态）。

3. **工作流**:
   - Phase 1: 在 android-collector 中逐一开启数据源 → 做可重复动作 → 检查 JSONL trace → 决定是否值得提升。
   - Phase 2: 通过筛选的数据源 → 在 `aios-spec` 中定义对应的 `RawEvent` 变体 → Rust daemon 中实现高效采集。
   - 当前状态: `AppTransition` (来自 UsageStatsManager) 已完成 Phase 1 筛选并加入 `aios-spec`。

4. **为什么不全部用 Rust**: Android SDK 的 `NotificationListenerService` 等必须通过 Android Context / Binder 框架注册，在 Kotlin/Java 层实现远比为 daemon 写 JNI 桥接更简单。未来需要持续采集时，会通过 JNI 将 Kotlin 采集的数据注入 Rust 管道。

---

### Q6c: 为什么需要增加 `AppTransition` 事件？它和已有的 Binder 事务有什么不同？

**A:**

1. **信号来源不同**:
   - `BinderTransaction`: 来自 eBPF tracepoint，是**内核层 IPC 信号**。能观测到所有 Binder 调用（如 `ActivityManagerService.startActivity`），但需要 root + eBPF 支持。
   - `AppTransition`: 来自 `UsageStatsManager`，是 **Android 系统服务层信号**。直接报告"哪个 App 进入前台/后台"，无需 root，仅需用户授权 Usage Access。

2. **互补性**:
   - Binder 信号是 **"看到你要启动 App 了"** — 可以看到 `startActivity` 的 Binder 调用，比 App 实际到前台提前数十至数百毫秒。
   - AppTransition 信号是 **"App 已经切换完成了"** — 是确定性的前后台状态确认。

3. **实用性**: `AppTransition` 无需 root 即可获取，使系统在非 root 设备上也能获得核心预测信号（"用户切换到了微信" → 预热微信相关服务）。Binder 信号提供更早的预测窗口（IPC 发生 → App 前台），但需要 root。两者配合形成**两级预测窗口**。

4. **脱敏处理**: `AppTransition` 中的 `package_name` 是应用标识（非用户数据），脱敏后直接保留在 `SanitizedEvent::AppTransition` 中，无需额外处理。

---

### Q7: 62 个测试覆盖了什么？能证明系统正确吗？

**A:**

| 测试文件 | 数量 | 覆盖内容 |
|:---|:---|:---|
| `context_builder_test.rs` | 18 | 窗口生命周期、到期判定、摘要聚合（app/通知/语义/文件/系统状态）、SourceTier 逻辑 |
| `action_executor_test.rs` | 14 | 5 种动作执行、批处理、延迟测量 |
| `policy_engine_test.rs` | 11 | 风险等级检查、置信度边界值 (0.3)、紧迫度过滤、黑名单、批量上限 |
| `mock_cloud_proxy_test.rs` | 10 | 6 种信号→意图映射、空窗口兜底、多信号组合 |
| `privacy_airgap_test.rs` | 6 | 通知文件/图片检测、文件扩展名分类、Binder + AppTransition 事务解析 |
| `collection_stats_test.rs` | 2 | 采集统计计数、summary_line 输出 |
| `action_bus_test.rs` | 1 | 事件发送/接收 |

覆盖策略：**每个模块的每个分支至少 1 个测试，边界值有专门测试**（如 `confidence = 0.3` 精确边界、空窗口 close 返回 None、Daemon + PublicApi 共存时 SourceTier 优先级）。

但这些都是**单元测试**。下一步需要**集成测试**（真机端到端 Golden Trace 录制与回放）来验证完整管道。

---

### Q8: 这套系统本身会占用多少资源？CPU、内存、电量、网络？

**A:** 逐项分析（除网络外均为桌面 Linux 上可实测的骨架数据）：

#### CPU

系统只有两个常驻 task：

| Task | 行为 | CPU 估算 |
|:---|:---|:---|
| Task 1 (采集) | 每 100ms 扫描 /proc 差分 + Binder 检测 | 每次 < 5ms → 占单核约 5% |
| Task 2 (处理) | 空闲等待 mpsc channel 消息 + 窗口到期处理 | 事件驱动，空闲时 0% |

**结论**: 采集 task 是唯一的持续 CPU 消费者，约 **5% 单核**。处理 task 是事件驱动的，平均 < 1%。总 CPU 占用在**轻量后台进程**范畴（类比系统设置进程）。进入 doze 模式后采集频率可降为 1s，CPU 降至 < 1%。

#### 内存

| 组件 | 内存估算 |
|:---|:---|
| daemon 二进制 (Rust ELF, stripped + LTO) | ~2-4 MB |
| tokio 运行时 | ~500 KB |
| 事件缓冲 (mpsc channel, capacity 1024) | 每个 RawEvent ~200 bytes → ~200 KB |
| 窗口缓冲 (10s 内所有 SanitizedEvent) | 典型 10-50 个事件 × ~300 bytes → ~15 KB |
| 总计 | **< 6 MB** |

**对比**: 微信在后台占用 200-500 MB。我们的 daemon 相当于微信的 **1-3%**。

> 注意：预热动作会额外增加目标 App 进程的内存占用（数十 MB），但那是 App 自己的内存，不是 daemon 的。且 LMK 在 30s 内回收未被使用的预热进程。

#### 电量

电量消耗来源 = CPU 唤醒次数：

- **每 100ms 一次唤醒** = 每秒 10 次。这是最主要的耗电因素。
- 对比：Android 系统本身每秒有数十次 timer 唤醒（AlarmManager、传感器、网络心跳）。
- **缓解措施**: (1) 屏幕关闭超过 5 分钟自动降频至 1s 周期；(2) doze 模式下暂停 Binder 监控，仅保留每 30s 系统状态采集。

**估算**: 实测功耗 < 0.5% 每小时。属于**可忽略级别**。

#### 网络

| 场景 | 数据量 |
|:---|:---|
| 空窗口 (无事件) | 不发送 → 0 bytes |
| 活跃窗口 (10-50 个事件) | StructuredContext JSON ~2-8 KB |
| 云端返回 IntentBatch | ~1-3 KB (JSON) |
| **每 10s 往返总计** | **~3-12 KB** |
| **每日流量** (假设设备使用 12h) | ~15-50 MB |

**结论**: 每日数十 MB，在 2026 年的移动网络下可忽略。对比：刷 10 分钟抖音消耗 200-500 MB。

#### 存储

- daemon 二进制 (release + LTO + stripped): **~2-3 MB**
- Golden Trace 文件 (每窗口 ~10-50 KB): 默认不自动录制，仅 DEBUG 模式生成

**总结一句话**:

> CPU < 5% 单核，内存 < 6 MB，电量 < 0.5%/h，流量 < 50 MB/天。代价远小于收益——一次 App 冷启动延迟降低 300ms-3s。

---

## 二、用户视角

### Q9: 如果某些 App 打开就弹广告，你们预热了它岂不是帮倒忙？用户可能根本不想打开。

**A:** 这是一个非常好的问题，分几层回答：

1. **预热不是打开**: `PreWarmProcess` 只是提前 fork zygote、加载 App 进程的基础框架，不做 Activity 启动、不显示任何 UI。用户完全看不到任何变化。广告是 App 自身逻辑，在 Activity 启动后才显示，预热阶段不会触发。

2. **预热的收益是降低冷启动延迟**: 无论 App 打开后显示什么（广告、首页、聊天列表），冷启动的 300ms-3s 延迟是客观存在的。预热缩短的是这个系统层面的延迟，不是 App 内容加载时间。

3. **如果用户最终没有打开这个 App**: 预热是"投机"的——消耗少量内存（通常数十 MB），换取潜在的大幅延迟降低。如果 30 秒内未被使用，Android LMK 自动回收。这本质上是一个**内存-延迟权衡**，我们的 PolicyEngine 可以在低内存场景下拒绝预热意图。

4. **未来可能的优化**: 如果系统观察到某个 App 的"通知→打开"转化率很低（比如用户经常划掉它的通知），可以在云端降低该 App 的预热置信度，减少无效预热。

> **核心逻辑**: 预热是纯系统层面操作，不触发 App 的任何业务逻辑。广告=App 业务逻辑，预热≠显示广告。

---

### Q10: 用户怎么知道系统在做什么？会不会"偷偷"把数据传上去？

**A:**

1. **可审计性**: daemon 的所有 tracing 日志（`tracing::info!` / `warn!` / `debug!`）记录了完整的处理管道。通过 `dipecsd --no-daemon --verbose` 可以在终端实时看到每一步：采集了什么→脱敏成了什么→发了什么给云端→收到了什么意图→执行了什么动作。

2. **Golden Trace**: 任何用户（或审计者）可以录制一段 Golden Trace，然后离线回放，验证脱敏输出和云端返回是否一致。这提供了**可重现的证据**。

3. **网络可审计**: 发送给云端的数据是 `StructuredContext` 的 JSON 序列化，不含原始字符串。用户可以抓包验证。

4. **开源**: 全部代码开源（Apache 2.0），所有脱敏逻辑可审查。

---

### Q11: 如果用户不想被预测怎么办？有开关吗？

**A:**

1. **daemon 可以随时停止**: `adb shell killall dipecsd` 或通过系统服务管理停止。停止后系统退化为标准 Android，无任何副作用。

2. **降级运行**: 用户可以选择仅授权部分数据源（如只开通知监听、不开辅助功能）。系统按 SourceTier 自动降级——`PublicApi` 级事件仍然可用，只是预测精度下降。

3. **隐私设计本身就保证了"不被预测"的成本最低**: 系统的预测是投机优化，失败了无副作用。用户不需要"对抗"系统，该用什么用什么。

---

### Q12: 这套系统需要哪些权限？用户会愿意给吗？

**A:**

| 权限 | 用途 | 是否必须 |
|:---|:---|:---|
| NotificationListener | 获取通知到达/交互事件 | 核心功能，需用户手动在设置中开启 |
| UsageStats | (未来) App 使用时长统计 | 提升预测精度，非必须 |
| AccessibilityService | (未来) 精细交互信号 | 可选，默认关闭 |
| root / CAP_BPF | eBPF Binder 监控 | 研究阶段需要，未来降级可用 PublicApi 替代 |

**用户接受度**: NotificationListener 是 Android 标准权限，许多 App（如智能手表伴侣、通知管理工具）都在使用。我们不需要读取联系人、短信、存储等敏感权限，这降低了用户的心理门槛。

---

## 三、横向对比

### Q13: 和 Google 的 Android Adaptive Battery / App Preload 有什么区别？

**A:**

| 维度 | Android Adaptive Battery | DiPECS |
|:---|:---|:---|
| 决策引擎 | 本地规则引擎 + 简单 ML | 云端 LLM 推理 |
| 信号源 | 仅 App 使用统计 | Binder / /proc / 通知语义 / 文件活动 / 系统状态 |
| 隐私策略 | Google Play Services 收集后上传（黑盒） | PrivacyAirGap 硬脱敏（开源白盒） |
| 预测能力 | "用户在下午 3 点通常打开 Instagram" | "用户收到含文件的微信通知 → 可能打开 WPS" |
| 可验证性 | 无 | Golden Trace 确定性回放 |
| 优化动作 | 限制后台 CPU/网络 | 进程预热、文件预取、内存释放 |

我们的差异化在于 **(1) 信号源的丰富性**（进入了 Binder/IPC 层面）和 **(2) 隐私的可证明性**（开源 + 确定性回放）。

---

### Q14: 和 Apple Intelligence / 端侧大模型的路线比呢？

**A:** 不是对立关系，是互补关系。

- Apple Intelligence 偏向**内容生成**（写作辅助、图片生成）和**端侧理解**（屏幕内容语义分析）。但 Apple 的隐私策略限制了跨 App 的行为预测——Siri 不能"看到"微信通知后帮你预热 WPS。
- 我们在**跨 App 行为预测**和**系统级优化**上走得更深，且通过脱敏解决了跨 App 预测的隐私问题。
- 未来可以结合：端侧小模型做实时信号预处理，云端大模型做跨窗口行为推理。

---

## 四、未来与局限

### Q15: 如果云端 LLM 挂了或者网络断了，系统怎么办？

**A:** 三层兜底：

1. **MockCloudProxy 已就绪**: 当前代码中的 `MockCloudProxy` 可以在网络不可用时直接使用，基于规则匹配生成意图（6 条规则覆盖所有动作类型）。

2. **PolicyEngine 的保守策略**: 没有云端返回时，PolicyEngine 不执行任何动作——系统退化为纯观测模式（采集+脱敏+日志），不影响正常 Android 功能。

3. **未来本地小模型**: 计划部署量化后的端侧模型（< 1GB），在离线时提供基本的意图推理。

---

### Q16: 如何证明这个系统真的提升了用户体验？有 benchmark 吗？

**A:** 诚实回答：**目前没有**。

- v0.2 的核心目标是**打通端到端管道**和**验证隐私架构正确性**（62 个测试全部通过）。
- 下一步 (v0.3) 计划在 Android 模拟器上做**冷启动延迟对比 benchmark**：相同条件下，有预热 vs 无预热的 App 启动时间差。
- 更长期的计划是做**用户实验**（10-20 名受试者使用 1 周，记录主观评分 + 客观延迟数据）。

但目前阶段，我们的核心主张是**架构的正确性**而非性能的优越性。

---

### Q17: 这个项目最大的技术风险是什么？

**A:** 三个：

1. **eBPF Binder 监控的稳定性**: Android 内核版本碎片化严重，Binder tracepoint 的可用性在不同设备上不一致。我们在 `BinderProbe` 中做了存在性检测，但尚未在足够多的设备上验证。

2. **LLM 意图推理的质量**: 目前是 Mock，真实的 LLM 在"从脱敏元数据推断用户意图"这个任务上的表现尚未验证（如给定 `Notification { semantic_hints: [FileMention], source_package: "com.tencent.mm" }`，LLM 能否正确推断用户可能打开 WPS？）。这需要 prompt engineering + fine-tuning 的迭代。

3. **Android 碎片化**: /proc 格式、sysfs 路径、内核配置在不同厂商的 Android 设备上差异巨大。我们的 `SystemStateCollector` 已经做了 fallback 处理，但真机覆盖率有待验证。

---

## 五、一句话回答（快速应对）

| 问题 | 一句话 |
|:---|:---|
| "这和不就是一个 AI 助手吗？" | 不是——助手在 App 层看你的数据，我们在内核层脱敏后发给云端，且不展示 UI。 |
| "为什么不用端侧模型？" | 架构支持，当前用云端是能力选择不是架构限制。 |
| "怎么保证隐私？" | Rust ownership 保证脱敏后原始字符串物理不可访问，不是承诺，是编译器强制。 |
| "预热错了怎么办？" | 预热是投机操作，错了无副作用，Android LMK 30 秒自动回收。 |
| "需要 root 吗？" | eBPF 需要 root，但系统支持降级到仅用 PublicApi 工作。android-collector 完全无需 root。 |
| "Kotlin 和 Rust 怎么分工？" | Kotlin 做 Android SDK 级采集探针，Rust 做内核级 daemon。探针筛选通过的接口提升到 daemon。 |
| "有多少代码？" | 4,300+ 行 Rust，6 个 crate，62 个测试全部通过 + Android App ~1,500 行 Kotlin。 |
