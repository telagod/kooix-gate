# Kooix Gate Roadmap

> 先收口，后补全，最后打磨。主轴是 **渠道插件化**：用 manifest 直接吃下私有协议、认证差异、SSE 格式、字段映射与 usage 归一，形成 Kooix Gate 真正的竞争力。

## 当前基线（2026-05-18）

`main` 已具备可用网关底盘：

- 多 Org / Project / ApiKey 三层租户，RBAC + RLS 双闸隔离。
- 9 个编译期 Provider：OpenAI / Anthropic / Azure / Gemini / DeepSeek / Mistral / Groq / Moonshot / Bedrock。
- HTTP Plugin manifest + SSE normalizer，可接私有协议与非标准 SSE。
- Provider 插件预设：OpenAI-compatible / Anthropic Messages / Azure OpenAI / Gemini / DeepSeek / Mistral / Cohere / Ollama / Groq / Together / OpenRouter / Moonshot / 智谱 / 通义千问 / 零一万物 / Bedrock Converse。
- OpenAI-compatible `/v1/chat/completions`，含 streaming / non-streaming / tool calling。
- Channel group 路由：priority / weighted_random / round_robin / least_conn / least_latency，含 fallback group。
- 多维度定价：`pricing_rules` + LiteLLM 自动同步 + REST / CLI / UI 管理面。
- Quota：rpm / tpm / budget，Redis Lua 原子执行，budget pre-debit 已具备 inflight crash recovery。
- typed ID API response + `FlexUuid` path 兼容。
- SvelteKit 控制台：Channel、Group、Pricing、Quota、Usage、Requests、Billing、SSO、Users 等管理面。
- 前端设计模板：`PageShell` / `SectionCard` / `DataToolbar` / `DataTable` 等。
- CI：Rust fmt / clippy / check / tests + Web build；当前文档记录 277 Rust test list entries（272 unit/integration + 5 doctest）+ 55 web tests。

## 战略主线：渠道插件化

Kooix Gate 不能只做“又一个 OpenAI-compatible proxy”。真正护城河是：**新增渠道优先不写 Rust adapter，而是写 manifest**。

渠道插件化要解决的痛点：

- **私有协议**：不同 path、method、query、body、message 结构、tool calling 结构、模型名映射。
- **认证差异**：Bearer、API key header/query、Basic、HMAC 签名、OAuth client credentials、AWS SigV4、厂商自定义 header。
- **响应字段映射**：content、tool_calls、finish_reason、usage、cache token、request id、错误码都可声明式抽取。
- **SSE 格式混乱**：CRLF/LF、注释、多行 `data:`、私有 event、嵌套 token、usage 末帧、`[DONE]` / `EOF` / heartbeat 都归一成 OpenAI-compatible chunk。
- **运营闭环**：manifest 不只是能请求，还要能 probe、计费、限流、观测、错误归类、回放测试。

竞争力定义：

1. 接一个普通 OpenAI-compatible 私有渠道：**5 分钟内**完成，无需发版。
2. 接一个 body/SSE/usage 都非标的私有渠道：**30 分钟内**完成，可在 UI 预览映射并生成回放 fixture。
3. 新渠道接入不破坏租户隔离、密钥加密、quota、billing、request log、health/fallback。
4. 编译期 Provider 逐步收敛为“高性能内置 preset”，运行时 HTTP Plugin 成为默认扩展面。

## 路线总览

| 阶段 | 目标 | 结果定义 |
| --- | --- | --- |
| P0 收口 | 把现有能力封成稳定可发版本 | 文档、迁移、测试、部署、回滚、兼容边界全部对齐 |
| P1 补全能力 | 以渠道插件化为主轴补齐运营网关闭环 | Plugin manifest / 认证 / 字段映射 / SSE / 计费 / 配额 / 观测完整 |
| P2 打磨 | 从“能用”打到“好用、稳、快、可卖” | UX、性能、DX、可维护性、演示与发布资产成熟 |

---

## P0 — 收口：冻结边界，斩断漂移

### P0.1 版本与文档收口

**目标**：让仓库状态、文档、CHANGELOG、README、DESIGN、CLI README 完全一致。

- [x] 决定下一版号：`v0.2.0`。
  - 建议：若只作为补丁发布，用 `v0.1.6`；若将 Provider preset + typed ID + pricing CRUD 视为新产品面，用 `v0.2.0`。
- [x] 将 `CHANGELOG.md` 的 `[Unreleased]` 落为正式版本段。
- [x] README 的“当前版本”与 badge、测试数、核心能力同步。
- [x] `DESIGN.md` 中路线图与真实实现保持一致，避免已完成事项继续显示为 TODO。
- [x] 为 HTTP Plugin manifest 写一页可复制示例：
  - OpenAI-compatible
  - Anthropic Messages
  - Azure OpenAI deployment path
  - 私有 SSE token frame

**验收门禁**

```bash
git diff --check
rg 'TODO|待下版本|尚未接入|返裸 UUID|24 migrations|241 tests' README.md DESIGN.md crates/kgctl/README.md web/README.md
awk '/^## \[0.2.0\]/{flag=1} /^## \[0.1.5\]/{flag=0} flag' CHANGELOG.md | rg 'TODO|待下版本|尚未接入|返裸 UUID|24 migrations|241 tests'
```

### P0.2 迁移与数据库收口

**目标**：数据库从空库迁移、旧库迁移、测试库迁移都可重复执行。

- [x] 全量验证 25 个 migration 空库可跑通。
- [x] 验证 v0.1.5 数据库升级到 v0.2.0：
  - `pricing_rules` 旧数据迁移。
  - `inflight_requests.quota_keys` / `estimated_micros` 默认值正确。
  - typed ID 不改变 DB 裸 UUID 存储。
- [x] 明确 v0.2.0 暂不提交 `.sqlx` 离线产物：当前仓库未启用 `SQLX_OFFLINE` 且没有 `query!` 宏，CI 以 `cargo check/test` + migration 测试兜底。
- [x] 明确 TimescaleDB 可选依赖：v0.2.0 默认普通 PostgreSQL 15+ 可运行，高吞吐生产建议将 `usage_records` 升级为 TimescaleDB hypertable。

**验收门禁**

```bash
cargo run -p kgctl -- migrate --dry-run
cargo test -p gate-storage --test pg_repo
cargo test -p gate-storage --test rls_isolation
```

### P0.3 测试与 CI 收口

**目标**：CI 能代表真实发布质量，不靠人工记忆。

- [x] CI Web job 增加 `npm run check` 与 `npm test`，不只 build。
- [x] CI 增加 `git diff --check`。
- [x] 明确 Docker / testcontainers 的服务版本：
  - Postgres `17-alpine`
  - Redis `7-alpine`
- [x] 把当前“Node.js 20 deprecated annotation”处理掉：
  - CI 显式 `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24=true`，Web job 使用 Node 22；若第三方 action 仍吐 annotation，作为非阻断噪音跟随 action 升级处理。
- [x] 建立 smoke test runbook：`RELEASE.md` 固化 compose config、依赖启动、migrate、doctor、admin create、server 启动与发布后 artifact 核验；自动化 `kgctl smoke` 留到 P2.5。

**验收门禁**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build
```

### P0.4 发布与回滚收口

**目标**：任何一次发布都有版本、镜像、迁移、回滚说明。

- [x] 补 `RELEASE.md`：
  - 发布命令
  - migration 前置检查
  - Docker image tag 规则
  - 回滚策略
  - 事故联系人 / runbook 链接
- [x] `docker-compose.yml` 与 `docker-compose.dev.yml` 核对端口、健康检查、env。
- [x] `kgctl doctor` 补充更多部署前检查：
  - `KOOIX_PUBLIC_URL`
  - JWT secret 长度
  - master key base64 32B
  - DB migration 版本
  - Redis Lua 可执行性
- [x] 发布 tag 后确认 GitHub Release artifact：
  - `v0.2.0` tag 指向 `4deb836`。
  - GitHub Release 已发布：`https://github.com/telagod/kooix-gate/releases/tag/v0.2.0`。
  - Docker workflow `25998915274` 成功，GHCR 推送 `v0.2.0` / `latest`，manifest digest `sha256:69b9b499f2bfc74dbce77838358bfe7245aac4fa3eedcfdd64dcecedeeed7832`。

**验收门禁**

```bash
docker compose config
docker compose -f docker-compose.dev.yml config
cargo run -p kgctl -- env
cargo run -p kgctl -- doctor
```

### P0.5 安全收口

**目标**：把现有高风险点先封住，不等功能继续膨胀。

- [x] 全仓 secret scan，确认测试 key / token 不会误入 release。
- [x] 确认所有 admin mutation 都走 `Permission::PlatformAdmin` 或对应 scope。
- [x] 核查 channel key / OIDC client_secret AAD 绑定一致性。
- [x] 补安全 runbook：
  - master key 丢失
  - JWT secret 轮换
  - channel key 泄露
  - Redis quota 计数异常
- [x] HTTP Plugin manifest 作为不可信配置处理：
  - 禁止 SSRF 到内网元数据地址。
  - 限制 header 模板可用变量。
  - 限制 request body / response body 大小。

**验收门禁**

```bash
rg 'unwrap\\(|expect\\(|TODO|FIXME|password|secret|token|sk-' crates web --glob '!target' --glob '!node_modules'
rg 'require!|Permission::PlatformAdmin|Scope::Platform' crates/gate-server/src/routes
```

### P0.6 渠道插件化收口

**目标**：把现有 HTTP Plugin 从“能用”封成“可承诺的扩展边界”。这是下一阶段的主战场。

- [x] 冻结 `plugin manifest v0` 现状：
  - `request.chat_path` / `headers` / `body` 模板变量。
  - `response.content_path` / `finish_reason_path` / `usage.*_path`。
  - `stream.event_path` / `content_path` / `finish_reason_path` / `done` / `usage.*_path`。
  - `preset.provider` 当前兼容列表。
- [x] 写 `docs/plugin-manifest.md`，明确当前支持与不支持：
  - dot path 抽取能力边界。
  - 模板变量白名单。
  - 密钥只能来自 `channel_keys` / env fallback，不允许 manifest 内明文落密。
  - streaming 与 non-streaming 的 usage 归一规则。
- [x] 建立私有协议 golden fixture：
  - 非 OpenAI body。
  - 自定义 auth header。
  - 非标准 JSON response。
  - 非标准 SSE token frame。
  - usage 末帧 / 无 usage / 分片 UTF-8。
- [x] 给 preset 与自定义 manifest 增加兼容性测试矩阵，避免后续 schema v1 破坏旧配置。

**验收门禁**

```bash
cargo test -p gate-providers plugin
cd web && npm test -- plugin-presets
rg 'plugin' README.md DESIGN.md CHANGELOG.md web/README.md ROADMAP.md
```

---

## P1 — 补全能力：以渠道插件化为主轴，把网关闭环补齐

### P1.1 Channel Pluginization 核心化

**目标**：把渠道接入从“写代码适配 Provider”升级为“写 manifest 接入协议”。这是 Kooix Gate 的第一竞争力。

#### P1.1.1 Manifest schema v1

- [x] 定义 `plugin.version = 1`，保留 v0 自动升级路径。
- [x] Manifest 顶层分区固定：
  - `metadata`：name、vendor、homepage、docs、owner、tags。
  - `capabilities`：chat、streaming、tools、embeddings、image、audio、vision、json_mode、batch。
  - `auth`：认证策略，不允许明文 secret。
  - `request`：method、path、query、headers、body、timeout、retry。
  - `response`：非流式字段映射。
  - `stream`：SSE / chunked streaming 映射。
  - `usage`：token / image / audio / cache / batch 归一规则。
  - `error`：状态码与错误 body 映射。
  - `probe`：健康检查与模型探测。
  - `security`：出站 allowlist、大小限制、header redaction。
- [x] 提供 JSON Schema，用于后端校验、前端表单、CLI lint 共用。
- [x] `model_mapping.plugin` 继续作为存储入口，但内部解析为强类型 manifest，错误信息带 JSON pointer。

#### P1.1.2 认证插件化

- [x] 内置基础认证策略：
  - `bearer`：`Authorization: Bearer {{api_key}}`。
  - `api_key_header`：如 `X-Api-Key` / `api-key`。
  - `api_key_query`：query 参数签发，默认高风险提示。
  - `basic`：username/password 来自 encrypted channel key material。
  - `custom_headers`：仅允许白名单变量。
- [x] Secret 来源统一：`channel_keys` envelope encryption / env fallback；manifest 只引用 secret slot，不存明文。
  - `channel_keys.label` 归一为 secret slot，`primary` / `api_key` 兼容旧主密钥。
  - Plugin runtime 会把同一 channel 的 active key 解密成 slot map；非 plugin provider 仍只取 primary。
  - DB 无 key、repo/crypto 未配置或本地开发时回退 `KOOIX_CH_<CODE>_KEY` / `KOOIX_API_KEY` / `KOOIX_PLUGIN_SECRET_<SLOT>`。
- [x] 内置 `hmac` 高级认证策略：
  - method / path / query / body_sha256 / timestamp / nonce 可组合签名 payload。
  - 默认 `HMAC-SHA256`，支持 hex / base64 signature header。
  - 自动注入 timestamp / nonce / signature header，secret 仍只来自 `secret_slot`。
- [x] 内置 `aws_sigv4` 高级认证策略：
  - canonical request / string-to-sign / signing key 按 AWS Signature Version 4 生成。
  - 自动注入 `Authorization`、`x-amz-date`、`x-amz-content-sha256`，可选 `x-amz-security-token`。
  - Bedrock Converse preset 默认使用 `aws_sigv4`，不再注入临时 `X-Amz-Access-Key` / `X-Amz-Secret-Key` header。
- [x] 内置 `oauth_client_credentials` 高级认证策略：
  - `oauth_client_credentials`：token cache + expiry refresh。
- [x] 前端创建 / 编辑 channel 时按 auth strategy 展示最小字段，保存前做本地 lint。

#### P1.1.3 Request 映射 DSL

- [x] 支持 path / query / header / body 模板：
  - `{{model}}`
  - `{{messages}}`
  - `{{last_user_message}}`
  - `{{stream}}`
  - `{{temperature}}` / `{{max_tokens}}` / `{{top_p}}`
  - `{{tools}}` / `{{tool_choice}}`
  - `{{metadata.*}}`
- [x] 支持 message transform：
  - OpenAI messages → vendor messages。
  - system prompt 合并 / 拆分。
  - multimodal parts 映射。
  - tool calls / tool results 映射。
- [x] 支持条件字段：参数为空时不发，避免私有渠道拒绝未知字段。
- [x] 支持 model alias / deployment path：Azure、Bedrock、私有 deployment 都走 manifest。

#### P1.1.4 Response / Usage 字段映射

- [x] 字段抽取从简单 dot path 扩展为稳定 path evaluator：
  - nested object
  - array index
  - first non-null fallback
  - literal default
- [x] 非流式 response 映射：
  - id
  - model
  - content
  - reasoning content（可选）
  - tool_calls
  - finish_reason
  - request_id / upstream metadata
- [x] Usage 归一：
  - prompt tokens
  - completion tokens
  - total tokens
  - cached tokens
  - image units
  - audio seconds
  - vendor 原始 usage metadata 保留。
- [x] 字段缺失时区分：可估算 / 不可计费 / 上游错误。

#### P1.1.5 SSE normalizer 产品化

- [x] 将现有共享 SSE decoder 上升为 manifest-driven normalizer：
  - CRLF / LF
  - comment / heartbeat
  - 多行 `data:`
  - chunked UTF-8
  - `event:` 分流
  - `[DONE]` / `EOF` / vendor done object
- [x] 支持私有 token 帧映射：
  - token path
  - role path
  - finish reason path
  - tool call delta path
  - usage 末帧 path
- [x] SSE replay harness：上传一段原始 SSE，UI 直接预览归一后的 OpenAI-compatible chunks。
- [x] 流式计费门禁：没有 usage 末帧时进入估算或标记不可计费，不允许静默漏扣。

#### P1.1.6 Error / Retry / Health 映射

- [x] Error mapper：
  - upstream auth → normalized `authentication_error`。
  - upstream rate limit → `rate_limit_error` + retry-after。
  - model not found → `invalid_request_error`。
  - vendor safety block → policy / content filter error。
  - unknown 5xx → retryable upstream error。
- [x] Manifest 可声明 retryable status/code、cooldown、circuit breaker 触发条件。
- [x] Probe 可声明轻量模型、请求体、成功条件、最大成本。
- [x] Health 结果进入 channel 状态、fallback、observability。

#### P1.1.7 Manifest Builder / Debugger

- [x] UI builder 分步创建：
  1. 选择 preset 或自定义。
  2. 配置 auth。
  3. 配置 request mapping。
  4. 粘贴 non-stream response sample，点选字段映射。
  5. 粘贴 raw SSE sample，预览 chunks。
  6. Test connection。
  7. 保存 channel 并加入 group。
- [x] CLI：`kgctl plugin lint|test|replay|export|import`。
- [x] 每个 manifest 自动生成 golden fixture，后续升级 schema 时回放验证。

**验收门禁**

```bash
cargo test -p gate-providers plugin_manifest
cargo test -p gate-providers sse
cargo test -p gate-server --test channel_plugin_e2e
cd web && npm test -- plugin-presets
```

### P1.2 Provider 能力矩阵

**目标**：让 plugin/preset/编译期 Provider 都能声明能力，路由、UI、计费按能力做决策。

- [x] 建立 `ProviderCapability`：
  - chat
  - streaming
  - tool calling
  - embeddings
  - image generation
  - audio STT/TTS
  - vision input
  - JSON mode / structured output
  - batch
- [x] 控制台显示 Provider / Channel capability，创建 channel 时提示不可用能力。
- [x] Provider preset 增加能力默认值与 base_url 建议。
- [x] 补齐 OpenAI-compatible 常见变体：
  - [x] vLLM
  - [x] LM Studio
  - [x] Ollama OpenAI endpoint
  - [x] LocalAI
  - [x] Xinference
- [x] 对 Bedrock Converse 用 plugin auth `aws_sigv4` 补齐正式鉴权。

**验收门禁**

- 每个 Provider/preset 至少有：
  - request adapter test
  - non-stream response test
  - stream response test
  - error mapping test

### P1.3 API 兼容面补全

**目标**：提升 OpenAI-compatible 覆盖度，减少迁移成本。

- [x] `/v1/models` 聚合真实 channel capabilities。
- [x] `/v1/embeddings` 路由闭环补强：
  - pricing
  - quota
  - request log
  - usage record
  - routed model / channel_id
  - provider error shape
- [x] `/v1/images/generations` 接入 provider adapter 与计费：
  - routed model / channel_id
  - pricing / billing outbox
  - quota pre-debit / settle
  - request log / usage record
  - provider error shape
- [x] `/v1/audio/transcriptions` / `/v1/audio/speech` 接入 provider adapter 与计费：
  - routed model / channel_id
  - TTS `per_character_tts` / STT `per_request` billing
  - quota pre-debit / settle
  - request log / usage record
  - provider error shape
- [x] 评估 `/v1/responses`：
  - [x] 已按 OpenAI 新 API 做 thin adapter 到 chat。
  - [x] 保持轻量：支持 string / item-array input、instructions、stream、tools、tool_choice、max_output_tokens。
  - [x] 不复刻完整 tool/state machine。
- [x] 统一 error shape：
  - [x] upstream auth → `authentication_error`。
  - [x] rate limit → `rate_limit_error` + `Retry-After` / `retry_after_ms`。
  - [x] quota exceeded → `quota_exceeded` / `quota_error`。
  - [x] model not found → `model_not_found`。
  - [x] no healthy channel → `no_healthy_channel`。

**验收门禁**

```bash
cargo test -p gate-server --test chat_e2e
cargo test -p gate-server --test billing_e2e
cargo test -p gate-providers --all-targets
```

### P1.4 Routing / Health / Fallback 补全

**目标**：路由从“策略可用”升级到“生产可控”。

- [x] Health probe 标准化：
  - [x] 每 provider 默认 probe model。
  - [x] probe 成本上限。
  - [x] 成功率 / 延迟 / 错误码分桶。
- [x] least_latency 从内存指标升级为持久化滑窗或 Prometheus query。
- [x] fallback 策略可视化：
  - [x] group chain 图
  - [x] 循环检测
  - [x] fallback 命中率
- [x] Channel draining：
  - [x] 禁止新请求
  - [x] 等待 inflight 清空
  - [x] 可安全下线 key/channel
- [x] Canary routing：
  - [x] 某 channel 只吃 1%-5% 流量
  - [x] 自动比较错误率 / 延迟 / 单价

### P1.5 Billing / Pricing / Ledger 补全

**目标**：从“usage 记录”走向“可对账计费”。

- [x] 引入 ledger 事件模型：
  - [x] estimated debit
  - [x] actual settle
  - [x] refund
  - [x] manual adjustment
  - [x] invoice close
- [x] `usage_records` 与 billing ledger 对账任务。
- [x] Pricing conditions UI：
  - [x] JSON editor
  - [x] 常见条件模板：cache、image size、audio seconds、batch、region。
- [x] 月账单状态机：
  - [x] draft
  - [x] closed
  - [x] exported
  - [x] paid / waived
- [x] CSV / JSON export 增加签名摘要，方便审计。
- [x] 成本告警：
  - [x] 预算 50/80/100%
  - [x] 单请求异常高成本
  - [x] channel 单价缺失

### P1.6 Quota / Policy 补全

**目标**：配额从 rpm/tpm/budget 扩为完整 policy engine。

- [ ] 实现并启用 concurrent quota。
- [ ] lifetime budget / lifetime tokens。
- [ ] user × model / api_key × model 的精确策略 UI。
- [ ] quota dry-run 模式：
  - 只记录会不会拦截
  - 不实际拦截
- [ ] quota explain：
  - 命中了哪条规则
  - 当前消耗
  - 下次恢复时间
- [ ] Redis 计数与 PG usage 对账。

### P1.7 Identity / Enterprise 补全

**目标**：从内部 admin 可用，走向企业接入可用。

- [ ] 邀请流：
  - org invite
  - project invite
  - 过期 / 撤销
- [ ] SSO provider UI 完整化：
  - OIDC discovery
  - allowlist
  - auto-join role
  - redirect policy
- [ ] SCIM 评估：
  - 用户同步
  - group → role mapping
- [ ] Session 管理：
  - 查看活跃 refresh token
  - 单用户踢下线
  - 全局 JWT rotation。
- [ ] `JwtRing`：支持新旧两把 JWT secret 窗口期验证。

### P1.8 Plugin Ecosystem / WASM 补全

**目标**：HTTP Plugin 成为稳定扩展面后，再把生态和更强扩展能力打开。

- [ ] Manifest registry：
  - 官方 preset。
  - 社区 manifest。
  - 私有 manifest 导入/导出。
  - 版本、作者、签名、兼容范围。
- [ ] Manifest package 规范：
  - `manifest.json`。
  - `fixtures/` 请求、响应、SSE 样本。
  - `README.md` 接入说明。
  - `security.md` 风险声明。
- [ ] Plugin sandbox 安全边界产品化：
  - SSRF denylist / allowlist。
  - DNS rebind 防护。
  - header redaction。
  - request / response size limit。
  - timeout / retry / circuit breaker。
  - manifest 权限声明。
- [ ] WASM 插件 ABI 设计稿只做 vNext：
  - request transform。
  - response transform。
  - streaming transform。
  - secret access API。
  - deterministic execution constraints。
  - 资源限制与审计。

### P1.9 Observability / Operations 补全

**目标**：生产出问题时能定位、能止血、能复盘。

- [ ] Prometheus metrics 完整命名：
  - request count
  - latency histogram
  - upstream error by provider/channel/model
  - quota deny
  - billing settle lag
  - outbox lag
- [ ] Trace 串联：
  - request_id
  - org/project/api_key/channel/model
  - upstream request span
  - billing/outbox span
- [ ] 控制台事故页：
  - 最近错误
  - top failing channels
  - quota deny top
  - upstream 401/429/5xx 分类
- [ ] Runbook：
  - 上游全挂
  - Redis 不可用
  - Postgres 慢查询
  - pricing sync 失败
  - outbox backlog。

---

## P2 — 打磨：从能用到好用

### P2.1 前端体验打磨

**目标**：控制台像产品，不像内部工具。

- [ ] 全页面套模板一致性审计：
  - header
  - toolbar
  - filter
  - table
  - empty / loading / error
- [ ] 表格能力统一：
  - server-side pagination
  - sort
  - column visibility
  - saved filters
- [ ] Channel 创建 wizard：
  - 选择 Provider / preset / 自定义 manifest
  - 选择 auth strategy 并填写 secret slot
  - 填 base_url / key / path template
  - 粘贴 response / SSE sample 并点选字段映射
  - 自动 probe
  - 保存并加入 group
- [ ] Pricing wizard：
  - 选择模型
  - 选择计费维度
  - 预览价格
  - 模拟一条 usage cost
- [ ] Quota wizard：
  - 选择 scope
  - 选择 model filter
  - 输入 rpm/tpm/budget
  - explain 预览。
- [ ] UI 文案统一：
  - 中文为主
  - Provider / Channel / API Key 等术语保留英文。

### P2.2 性能打磨

**目标**：稳定承载高并发，不让计费和日志拖慢主链。

- [ ] 路由 hot path benchmark：
  - provider selection
  - key decrypt cache
  - quota check
  - request log enqueue
- [ ] Channel key 解密缓存：
  - TTL
  - revoke 失效
  - rotation 失效
- [ ] Usage/outbox batch insert。
- [ ] Request log 分区 / retention。
- [ ] SSE parser 压测：
  - 小帧多
  - 大帧
  - 分片 UTF-8
  - 长连接取消。
- [ ] Web bundle 预算：
  - route-level splitting
  - flow editor lazy load
  - markdown highlighter lazy load。

### P2.3 安全打磨

**目标**：默认安全，且安全决策有证据。

- [ ] Threat model 文档：
  - tenant isolation
  - API key leakage
  - malicious plugin manifest
  - SSRF
  - billing fraud
  - admin account takeover
- [ ] 细粒度 audit：
  - before/after diff
  - actor subject
  - request_id
  - ip/user-agent
- [ ] Secret redaction 全链路测试。
- [ ] Admin 高危操作二次确认：
  - delete channel
  - rotate/revoke key
  - suspend user
  - change pricing
  - disable group
- [ ] Master key rotation tool：
  - dry-run
  - re-encrypt
  - verify
  - rollback plan。

### P2.4 DX / SDK / 示例打磨

**目标**：让用户 10 分钟内接入，维护者 10 分钟内定位问题。

- [x] `examples/`：
  - OpenAI SDK 直连
  - curl streaming
  - Provider preset channel create
  - custom HTTP Plugin manifest
  - 私有 auth + 字段映射 + SSE normalizer 示例
  - pricing rule create
  - quota create
- [x] OpenAPI spec 导出。
- [x] Postman / Bruno collection。
- [x] Terraform / Helm 示例。
- [x] `kgctl doctor --json` 给 CI / deploy pipeline 使用。
- [x] `kgctl smoke`：
  - 登录
  - 创建 channel
  - 创建 API key
  - 发 chat
  - 查 usage。

### P2.5 发布资产打磨

**目标**：每次 release 都能被外部用户理解和复现。

- [ ] Release checklist 固化到 `RELEASE.md`。
- [ ] GitHub Release 自动生成：
  - changelog
  - Docker image tag
  - migration notes
  - known limitations
- [ ] Demo script：
  - docker compose up
  - 创建 admin
  - 创建 provider preset channel
  - 发一条 chat
  - 看 usage / billing。
- [ ] 截图与短视频：
  - Dashboard
  - Channel wizard
  - Pricing rules
  - Request logs
  - Playground。

---

## 建议执行顺序

### 第一轮：收口版本（1-2 天）

1. 冻结现有 HTTP Plugin manifest v0 边界，补 `docs/plugin-manifest.md`。
2. CI 加 `web check/test` 与 `git diff --check`。
3. `CHANGELOG` 落版，写 `RELEASE.md`。
4. 迁移 / docker compose / kgctl doctor 做一轮 fresh install。
5. 安全 quick scan：secret / permission / plugin SSRF 风险。
6. 打 tag 发布。

### 第二轮：渠道插件化核心战（3-5 天）

1. Manifest schema v1 + v0 upgrade。
2. Auth strategy：bearer / api_key_header / api_key_query / basic / hmac / oauth / aws_sigv4。
3. Request / response / usage / error mapper 强类型化。
4. SSE replay + normalizer preview。
5. `kgctl plugin lint|test|replay`。

### 第三轮：运营闭环（3-5 天）

1. Channel manifest wizard + provider capability 矩阵。
2. Pricing conditions UI + quota explain。
3. Health / fallback / canary 可视化。
4. Observability dashboard + runbook。

### 第四轮：企业能力（1-2 周）

1. Invite flow + SSO UI 完整化。
2. Ledger / invoice 状态机。
3. JwtRing + master key rotation。
4. OpenAPI spec + examples + Helm/Terraform。

### 第五轮：插件生态（2-4 周）

1. Manifest registry / package / signed import。
2. Manifest builder 进阶：字段点选、SSE mapper preview、golden fixture。
3. Plugin sandbox 防 SSRF / 限资源 / 权限声明。
4. WASM ABI 设计与 PoC。

---

## 每轮固定门禁

每个阶段结束前必须过：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build
git diff --check
```

涉及 migration 时额外过：

```bash
cargo clean -p gate-storage
cargo test -p gate-storage --test pg_repo
cargo test -p gate-server --test auth_flow
```

涉及安全 / 权限时额外过：

```bash
rg 'fn.*\\(.*AuthContext' crates/gate-server/src -A 5 | rg -v 'require!|can!|require_user!|require_api_key!'
rg 'password|secret|token|sk-' . --glob '!target' --glob '!web/node_modules' --glob '!Cargo.lock'
```

---

## 非目标（暂不做）

- 不急于做完整 WASM 插件生态；HTTP Plugin manifest v1 和 manifest builder 先稳定一版。
- 不急于继续堆编译期 Provider；优先把主流 Provider 与私有协议都收敛到插件化 manifest 接入面。
- 不急于复制 OpenAI 全量 Responses API 状态机；先保证 Chat/Embeddings/Image/Audio 的路由、计费、日志闭环。
- 不急于引入复杂 ABAC 引擎；现阶段 RBAC + scope 足够。
- 不把 UI 装饰色扩成彩虹体系；继续 zinc-only + 语义色。

## 成败线

### 可发版线

- fresh install 30 分钟内跑通。
- admin 能创建 channel / key / pricing / quota。
- admin 能用 manifest 接入一个 OpenAI-compatible 私有渠道。
- API key 能成功发 chat，usage 与 billing 有记录。
- CI 全绿，文档无漂移。

### 可运营线

- 任一非标私有渠道能通过 manifest 映射 request / auth / response / SSE / usage，无需改 Rust。
- 任一 channel 出错可被观测、降级、下线。
- 任一用户/项目超额能解释为什么被拦。
- 任一账单能追溯到 usage 与 pricing rule。
- 任一高危操作有 audit。

### 可打磨线

- 新用户不看源码也能接入一个 Provider 或私有协议渠道。
- 新维护者不问人也能发布、回滚、排障。
- 新 Provider 不改核心路由也能接入；复杂私有 SSE 可用 replay fixture 验证。
