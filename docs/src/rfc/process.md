# RFC 提案

DiPECS 采用 RFC（Request for Comments）机制管理重大设计变更。所有跨模块接口变更、新功能提案、架构调整均需先提交 RFC 并获得批准。

## RFC 流程

1. **Fork & Branch**：从 `main` 分支创建 `rfc/XXXX-description` 分支
2. **Draft**：复制 `docs/templates/rfc/0000-template.md`，按模板撰写 RFC 文档，放入 `docs/src/rfc/`（可选：创建独立目录）
3. **Discuss**：提交 PR，在 PR 评论区进行技术讨论
4. **Approve**：至少 2 位核心成员批准
5. **Implement**：批准后按 RFC 内容实施

## 已批准的 RFC

- [RFC-0001: 分层采集与决策路由重构](0001-layered-collection-and-decision-routing.md)

## 草案中的 RFC

- [RFC-0002: Action Bus 治理边界与生命周期审计](0002-action-bus-governance.md) — 待评审（issue #4 / #5 / #8）

## 参考

- [Rust RFC 流程](https://rust-lang.github.io/rfcs/)
- [TensorFlow RFC 流程](https://github.com/tensorflow/community/tree/master/rfcs)
