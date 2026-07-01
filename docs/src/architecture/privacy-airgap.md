# 隐私脱敏

> Status: Current  
> Last verified: 2026-07-01  > Code anchors: `crates/aios-core/src/privacy_airgap.rs`, `crates/aios-core/src/text_analysis.rs`, `crates/aios-spec/src/event.rs`, `crates/aios-spec/src/sanitized.rs`

**这篇文档回答什么**：每种 `RawEvent` 经过 `PrivacyAirGap` 后变成什么、哪些字段被保留、哪些被丢弃、以及文本/文件路径如何被抽象。  
**适合谁读**：需要新增事件类型、理解模型输入边界或调试脱敏后语义的人。

## TL;DR

`DefaultPrivacyAirGap` 把 `RawEvent` 转成 `SanitizedEvent`：

- 原始文本 → `TextHint` + `SemanticHint`。
- 文件路径 → `ExtensionCategory`。
- Binder 方法/payload → `InteractionType`。
- 通知 key/group/tag → 丢弃。

模型后端只接触 `StructuredContext`，永远看不到原始 PII。

## 变体映射表

| RawEvent | SanitizedEventType | 保留 | 丢弃 | 默认 `source_tier` |
| --- | --- | --- | --- | --- |
| `AppTransition` | `AppTransition` | package、activity、transition | — | `PublicApi` |
| `NotificationPosted` | `Notification` | package、category、channel、`TextHint`、`SemanticHint`、ongoing | `raw_title`、`raw_text`、`group_key` | `PublicApi` |
| `NotificationInteraction` | `Notification` | package | notification key、action、title/text hint 清零 | `PublicApi` |
| `ScreenState` | `Screen` | state | — | `PublicApi` |
| `SystemState` | `SystemStatus` | battery、charging、network、ringer、location、headphone | `bluetooth_connected` | `PublicApi` |
| `ProcStateChange` | `ProcessResource` | pid、package、rss/swap MB、threads、oom_score | 原始 KiB、IO、state | `Daemon` |
| `FileSystemAccess` | `FileActivity` | `ExtensionCategory`、`FsActivityType` | 完整路径、bytes、pid | `Daemon` |
| `BinderTransaction` | `InterAppInteraction` | target_service、`InteractionType`、uid | method、pid、oneway、payload | `Daemon` |

## 文本分析

### `TextHint`

- `length_chars`：字符数。
- `script`：`Latin`、`Hanzi`、`Cyrillic`、`Arabic`、`Mixed`、`Unknown`。
- `is_emoji_only`：是否仅由 emoji 组成。

### `SemanticHint`

从 title + body 文本中提取：

| SemanticHint | 触发关键词示例 |
| --- | --- |
| `FileMention` | file、pdf、doc、zip、附件、文件 |
| `ImageMention` | image、jpg、png、截图、照片 |
| `AudioMessage` | voice、mp3、录音、语音 |
| `LinkAttachment` | http、https、url、链接 |
| `UserMentioned` | @你、@所有人、mentioned you |
| `CalendarInvitation` | meeting、calendar、会议、日程 |
| `FinancialContext` | payment、转账、红包、余额 |
| `VerificationCode` | code、otp、验证码、captcha |

如果 Android collector 已经提供了非空的 `semantic_hints`，Rust 侧直接使用，不再本地提取。

## 文件扩展名分类

| ExtensionCategory | 典型扩展名 |
| --- | --- |
| `Document` | pdf、doc、xls、ppt、txt、md、csv |
| `Image` | jpg、png、gif、webp、svg |
| `Video` | mp4、mov、avi、mkv |
| `Audio` | mp3、wav、flac、aac、ogg |
| `Archive` | zip、rar、7z、tar、gz、apk |
| `Code` | py、rs、js、kt、java、cpp |
| `Unknown` | 无扩展名 |
| `Other` | 其他 |

完整目录路径和文件名不会被保留。

## Binder 方法到 InteractionType

| 方法子串 | InteractionType |
| --- | --- |
| `enqueueNotification` | `NotifyPost` |
| `startActivity` / `startActivityAsUser` | `ActivityLaunch` |
| `share` / `sendIntent` | `ShareIntent` |
| 其他 | `ServiceBind` |

## source_tier 规则

- Android JSONL ingress：`PublicApi`。
- Rust 内部采集（`/proc`、system snapshot）：`Daemon`。
- 单个窗口内只要有一个 `Daemon` 事件，整个 `ContextSummary.source_tier` 提升为 `Daemon`。

## 新增 RawEvent 的检查清单

- [ ] 在 `aios-spec/src/event.rs` 添加类型
- [ ] 在 `aios-spec/src/sanitized.rs` 添加脱敏后变体
- [ ] 在 `aios-core/src/privacy_airgap.rs` 实现脱敏规则
- [ ] 添加 `privacy_airgap_test.rs` 单元测试
- [ ] 添加 `privacy_leak_test.rs` 子串扫描
- [ ] 添加 `privacy_airgap_property_test.rs` 属性测试
- [ ] 更新本文档映射表

## 相关文档

- [数据流](data-flow.md)
- [模型记忆与行为画像](model-memory.md)
- [Schema 参考](../refs/schemas.md)
- [Android 安全与隐私边界](../android/security-privacy.md)
