# gate-server

Axum HTTP 网关主二进制。承载 OpenAI-compatible data plane + 全部 admin / control plane API。

## Routes 大类

- **Data Plane**：`/v1/chat/completions` `/v1/embeddings` `/v1/images/generations` `/v1/audio/{speech,transcriptions}` `/v1/responses` `/v1/models`
- **Admin**：`/v1/admin/{channels,channel-groups,pricing-rules,users,identity-providers,audit-logs,wasm-modules,...}`
- **Org/Project scope**：`/v1/orgs/{orgId}/{projects,quotas,billing,invitations,...}`
- **Auth**：`/v1/auth/{login,refresh,sso/...}`
- **Public**：`/v1/setup` `/v1/invitations/{preview,accept}`
- **Observability**：`:9090/metrics`（Prometheus）+ OTLP via `KOOIX_OTLP_ENDPOINT`

## 关键中间件

- `tower::layer::Stack`：trace + cors + compression + request-id + auth extractor
- `AuthContext` extractor：JWT / API key 双入口归一为统一 context
- RLS session var 注入：每次 DB 事务前 `SET LOCAL kooix.org_id` 兜底
- Streaming：`axum::body::Body::from_stream` + `tokio_util::io::ReaderStream`，end-to-end backpressure

## 启动

```bash
cargo run -p gate-server --release
# 默认 :8080，metrics :9090，配置走 figment（toml + env）
```

详见 [docs/getting-started.md](../../docs/getting-started.md)、[docs/architecture/control-plane.md](../../docs/architecture/control-plane.md)、[docs/architecture/data-plane.md](../../docs/architecture/data-plane.md)。
