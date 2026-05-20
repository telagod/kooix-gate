# Data Plane

Status: active
Scope: `/v1/models`、`/v1/chat/completions`、`/v1/responses`、`/v1/embeddings`、`/v1/images/generations`、`/v1/audio/*` 的请求边界与执行链。
Last verified: 2026-05-21

## 职责

Data plane 负责：

- 认证上下文接收
- rate / quota / RLS 准入
- model alias 与 channel group 路由
- provider / plugin 适配
- streaming / non-streaming 执行
- usage、request log、billing outbox 生成

## 关键约束

- 不做平台管理 mutation。
- 不直接写复杂 projection。
- 不让 handler 里分散写 provider 特例。
- fail-open 只保留在明确允许的保护层，不把业务错误吞掉。

## 代码锚点

- `crates/gate-server/src/app.rs`
- `crates/gate-server/src/gateway.rs`
- `crates/gate-server/src/route_manifest.rs`
- `crates/gate-providers/src/router.rs`
- `crates/gate-server/src/routes/chat.rs`
- `crates/gate-server/src/routes/embeddings.rs`
- `crates/gate-server/src/routes/images.rs`
- `crates/gate-server/src/routes/audio.rs`
- `crates/gate-server/src/routes/responses.rs`
