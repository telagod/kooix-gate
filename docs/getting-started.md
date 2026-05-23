# Getting Started — Kooix Gate

> 三档接入：30 秒 docker compose / 5 分钟 Helm / 10 分钟本地源码。

## A. Docker Compose（30 秒，推荐试用）

```bash
git clone https://github.com/telagod/kooix-gate
cd kooix-gate
docker compose -f docker-compose.yml up -d
# 等 PostgreSQL / Redis ready
docker compose exec gate kgctl admin create --email admin@example.com --password 'PleaseRotate!'
open http://localhost:8080
```

第一次启动：

1. 自动跑全量 migration（35 个）
2. 默认 master key 从 `KOOIX_MASTER_KEY` env 读，未设则随机生成（仅 dev）
3. 控制台 `http://localhost:8080` 用 `admin@example.com` 登录

验证 chat：

```bash
curl http://localhost:8080/v1/chat/completions \
  -H 'Authorization: Bearer sk-xxxxxx' \
  -H 'Content-Type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}'
```

## B. Helm Chart（5 分钟，推荐生产）

```bash
helm repo add kooix-gate https://telagod.github.io/kooix-gate-charts  # TBD
helm install gate kooix-gate/gate \
  --set image.tag=v0.4.28 \
  --set master_key.fromSecret=kooix-master-key \
  --set postgres.dsn=postgres://gate@pg:5432/gate \
  --set redis.url=redis://redis:6379/0
```

详细 values 见 [deploy/helm/values.yaml](../deploy/helm/values.yaml)（0.4.31 完善）。

## C. 本地源码（10 分钟，开发用）

### 前置

- Rust 1.85+ (`rustup install 1.85`)
- Node 22+
- Docker（用于 testcontainers）

### 启动

```bash
# 终端 1：跑后端 + DB
docker compose -f docker-compose.dev.yml up -d
cargo run -p gate-server

# 终端 2：跑前端
cd web && npm install && npm run dev

# 浏览器：http://localhost:5173（前端）/ http://localhost:8080（后端 API）
```

### 创建第一个 channel + key + chat

```bash
# 1. doctor 检查环境
cargo run -p kgctl -- doctor

# 2. 创建 admin
cargo run -p kgctl -- admin create --email admin@example.com --password 'demo'

# 3. 控制台 → Channels → 新建（用 OpenAI preset，填 sk-...）
# 4. 控制台 → Projects → 新建 API Key
# 5. 用 API Key curl /v1/chat/completions
```

## 升级到 0.4.x 新功能

### WASM Plugin transform（ADR-0003 v0）

写一个 Rust transform：

```rust
// src/lib.rs
use gate_wasm_sdk::export_chat_request;

export_chat_request!(|body: &[u8]| -> Vec<u8> {
    // 你的 transform 逻辑
    body.to_vec()
});
```

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
gate-wasm-sdk = { git = "https://github.com/telagod/kooix-gate" }
```

```bash
cargo build --target wasm32-unknown-unknown --release
sha256sum target/wasm32-unknown-unknown/release/my_transform.wasm
```

manifest 配置（0.4.x 起 typed schema）：

```json
{
  "plugin": {
    "version": 1,
    "preset": { "provider": "openai_compatible" },
    "security": {
      "wasm": {
        "module": "modules/my_transform.wasm",
        "module_sha256": "<sha256-hex>",
        "max_memory_bytes": 16777216,
        "max_cpu_ms": 50,
        "hooks": ["chat_request_transform"]
      }
    }
  }
}
```

详细 ABI 见 [ADR-0003](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)。

### Manifest builder（UI 5 分钟接入私有渠道）

控制台 → Channels → 新建 → "HTTP Plugin" → preset 选 `openai_compatible` 或自定义 →
按 wizard 7 步走：preset / auth / request mapping / response sample 点选字段 /
SSE replay preview / probe / save。

## 故障排查

- `kgctl doctor` 检查环境（DB / Redis / master key / migration）
- `kgctl smoke` 端到端冒烟（创建 channel → key → chat）
- 详见 [docs/observability-runbook.md](./observability-runbook.md)

## 下一步

- [docs/architecture.md](./architecture.md) — C4 架构总览
- [docs/plugin-manifest.md](./plugin-manifest.md) — Manifest 完整 schema
- [ROADMAP.md](../ROADMAP.md) — 当前里程碑
