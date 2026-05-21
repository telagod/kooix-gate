# Data Plane

Status: active
Scope: `/v1/models`、`/v1/chat/completions`、`/v1/responses`、`/v1/embeddings`、`/v1/images/generations`、`/v1/audio/{speech,transcriptions}` 的请求边界与执行链。
Last verified: 2026-05-22

> 本页面是 data plane 的运行时边界与代码锚点。请求执行的完整论述见 [DESIGN.md §3-4](../../DESIGN.md)。

## 职责

Data plane 负责所有 **真实流量**：从 API key 校验、限流配额、模型路由、provider 调用、流式归一、计费写入到 request log 落盘的完整链路。

| 阶段 | 模块 |
|------|------|
| 1. 认证 | `gateway::auth_layer`（API key SHA-256 + AuthContext 装配） |
| 2. 限流 | `gateway::rate_limit`（in-memory token bucket） |
| 3. 配额 | `routes/quotas` middleware（Redis Lua 原子，rpm/tpm/concurrent/budget/lifetime） |
| 4. 路由 | `gate_providers::ProviderRouter`（priority/weighted_random/round_robin/least_conn/least_latency + fallback + canary） |
| 5. 解析 secret | `ProviderRouter::resolve_secrets`（channel_keys envelope decrypt + cache TTL） |
| 6. 上游调用 | `gate_providers::Provider` impl（OpenAI / Anthropic / Azure / Bedrock / CustomHttpProvider plugin runtime） |
| 7. 流式归一 | `gate_providers::sse::SseLineDecoder` + plugin manifest stream normalizer |
| 8. 计费 | `gate_billing::outbox::enqueue`（fail-closed estimated usage） |
| 9. request log | `gate_storage::request_events` + 月分区投影 |

## 关键约束

### 边界

- **不做平台管理 mutation**：所有 mutation 集中在 control plane。
- **不直接写复杂 projection**：仅写 outbox / `request_events` insert，复杂 rollup 留给 worker plane。
- **不让 handler 里分散写 provider 特例**：所有 vendor 差异收敛到 `Provider` trait 实现 + plugin manifest。
- **fail-closed billing**：上游缺失 usage 末帧时按 request message + `max_tokens` 生成 estimated usage，标记 `raw.estimated=true`，不静默跳过 billing/quota settlement。

### 路由

- **5 种策略**（[DESIGN §4.2](../../DESIGN.md)）：priority / weighted_random / round_robin / least_conn / least_latency
- **Canary**：`canary_percent_bps` 1-5%，路由热路径用 deterministic gate
- **Fallback**：`fallback_group_id` 形成 chain，禁止循环（max depth 5）
- **Capability gating**：chat/streaming/tools/vision/json_mode/embeddings/image/audio 不满足的 channel 跳过

### 错误归一

所有上游错误归一为 `NormalizedProviderErrorKind`：

| 上游 | 归一 | 客户端 shape |
|------|------|-------------|
| 401/403 + auth | `AuthenticationError` | `{error:{code,type:"authentication_error"}}` |
| 429 + rate limit | `RateLimit` | `{error:{...,type:"rate_limit_error"}}` + `Retry-After` |
| 404 + model missing | `ModelNotFound` | `{error:{...,type:"invalid_request_error",code:"model_not_found"}}` |
| 5xx | `Upstream` (retryable) | `{error:{...,type:"upstream_error"}}` |
| Plugin policy block | `Policy` | `{error:{...,type:"policy_error"}}` |
| Quota deny | `QuotaExceeded` | `{error:{...,type:"quota_error"}}` |
| 无 healthy channel | `RouteMiss` | `{error:{...,type:"no_healthy_channel"}}` |

### Crash-safe pre-debit

Budget quota 三段式（[DESIGN §3.3](../../DESIGN.md#33-流式扣费三段式)）：

1. **预扣**：Redis Lua 原子扣减 + `inflight_requests.quota_keys/estimated_micros` 落 PG
2. **结算**：上游 usage 末帧到达 → 多退少补
3. **崩溃恢复**：60s sweeper 扫 `inflight_requests`，超时则全退

## 代码锚点

- `crates/gate-server/src/app.rs`
- `crates/gate-server/src/gateway.rs` — auth + rate limit middleware
- `crates/gate-server/src/route_manifest.rs` — 路由清单
- `crates/gate-providers/src/lib.rs` — Provider / EmbeddingProvider / ImageProvider / AudioProvider trait
- `crates/gate-providers/src/router.rs` — ProviderRouter（M1.3 拆分中）
- `crates/gate-providers/src/custom_provider.rs` — Plugin runtime（M1.3 拆分中）
- `crates/gate-providers/src/plugin_manifest.rs` — Plugin manifest（M1.3 拆分中）
- `crates/gate-providers/src/sse.rs` — SSE decoder
- `crates/gate-server/src/routes/chat.rs`
- `crates/gate-server/src/routes/embeddings.rs`
- `crates/gate-server/src/routes/images.rs`
- `crates/gate-server/src/routes/audio.rs`
- `crates/gate-server/src/routes/responses.rs` — `/v1/responses` thin adapter
- `crates/gate-server/src/routes/models.rs` — `/v1/models` capability aggregation

## 跨页面交叉

- 上游与配置 → [Control Plane](./control-plane.md)
- 异步结算 → [Worker Plane](./worker-plane.md)
- Plugin 接入规范 → [docs/plugin-manifest.md](../plugin-manifest.md)
- 故障处置 → [docs/observability-runbook.md](../observability-runbook.md)
