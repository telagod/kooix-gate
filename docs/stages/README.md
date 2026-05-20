# 阶段性文档

本目录收纳已经完成的一次性审计、迁移、发布收口、验证快照和复盘记录。

规则：

- 文件名使用 `YYYY-MM-DD-topic.md`。
- 文档开头保留 `Status`、`Scope`、`Last verified` 等元数据。
- 阶段性文档可以保留历史证据和旧 TODO，但不得作为当前路线或实现状态的唯一入口。
- 连续推进同一阶段时优先追加到已有阶段文件，减少完成态文档碎片。
- 若内容变成长期运维规则，提炼到 `../README.md`、根目录 `README.md`、`DESIGN.md`、`ROADMAP.md` 或对应 runbook，再从阶段文档回链。

## 已归档阶段

| 文档 | 状态 | 用途 |
| --- | --- | --- |
| [2026-05-19-refactor-todo-audit.md](./2026-05-19-refactor-todo-audit.md) | implementation pass applied | Runtime / billing / DB / CI / observability 重构审计与完成证据。 |
| [2026-05-19-docs-and-secret-scan.md](./2026-05-19-docs-and-secret-scan.md) | applied | 文档分层清理、gitleaks 本地安装复验、Plugin secret slots、P1.8/P1.9/P2.2 后续收口、P2.3 安全打磨、P2.5 发布资产与全门禁证据。 |
