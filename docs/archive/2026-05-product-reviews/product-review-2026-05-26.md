# Kooix Gate 产品审查 · 2026-05-26

> 基线：`v0.4.64` / `main` / 11 crate workspace / 11 672 行 server / 26 web 路由 / 35 migrations / 485 Rust + 86 web tests
> 视角：批判 + 打磨。每条都给文件:行号、根因、动作。

---

## TL;DR — 一句话定性

**Kooix Gate 是一个"骨头很硬、关节缝里漏风"的产品**：
RBAC / 多租户 / 计费 outbox / 配额预扣 / WASM transform / channel routing 这些"难骨头"都啃下来了；但**热路径有结构性低效**（每 channel 一个 `reqwest::Client`、metric 几乎为零）、**god component 残留**（`channels/+page.svelte` 1252 行、`admin.rs` 4235 行）、**渠道一致性参差**（OpenAI 透传不映射工具语义、Bedrock/Anthropic 的 cache_*/reasoning tokens 路径不齐），**离"产品级"还差两口劲**。

---

## 0. 全局判词（按"会不会扎到客户"排序）

| # | 类别 | 判词 | 影响 |
|---|------|------|------|
| **P0-1** | 性能 | 每 channel/每次构建 `reqwest::Client::builder().build()`，连接池/keep-alive/HTTP2 全不复用 | 高并发下 connect 风暴、TLS 握手成本翻倍 |
| **P0-2** | 可观测 | `metrics::counter!/histogram!` 在 server 主路径几乎为 0；只有 worker 在打 metric | 出问题没数据，SLO/告警无从谈起 |
| **P0-3** | 一致性 | OpenAI provider 直接 `client.post().json(&req)` 透传，未做参数 sanitize；同时 Anthropic/Bedrock 已有 cache/reasoning 字段，路径不齐 | 上游加新字段 = 兼容面漂移 |
| **P0-4** | 复杂度 | `routes/admin.rs` 4235 行 / `web/src/routes/channels/+page.svelte` 1252 行 仍是 god file | 维护性、新人门槛、并发改动冲突 |
| **P0-5** | 安全 | provider error 中 `Upstream { body }` 直接带原始 body，未脱敏；error 透出可能含上游响应中的 key/header echo | 日志/审计可能泄漏敏感数据 |
| **P1-1** | 渠道 | OpenAI provider 没显式声明 capabilities；capability matrix 只在 plugin 路径走通 | 前端拿不到统一能力描述 → playground 节点联动落不下来 |
| **P1-2** | 渠道 | `from reqwest::Error` 把所有 401/403 都映射成 `Auth`，丢失 organization quota / billing 等 403 子类 | 用户拿到错误时无法区分"key 失效"和"额度耗尽" |
| **P1-3** | DB | 35 migration 已开始分区 `request_log_partition_retention`，但 `request_logs` 写路径在 hot path 上是 `Arc<dyn RequestLogRepo>` 直接 `insert`，未走 buffered/batch | 写放大，p99 抖动 |
| **P1-4** | WASM | host functions `host_log` / `host_get_secret_slot` / `host_record_metric` ABI 已设计但未实装（product-gaps G-003）| 插件无法 log / 拿 secret / 上报 metric，DX 残废 |
| **P1-5** | WASM | 编译产物未持久化（G-104），冷启动需要重新编译每个 wasm 模块 | 重启慢、容器扩容代价高 |
| **P2-1** | 前端 | `channels/+page.svelte` 1252 行虽已抽出 7 子组件，仍是"分配中心"——大量本地 state、API call、modal 协调 | 体验闭环但代码质量弱 |
| **P2-2** | 前端 | playground 路由本体 38 行（懒加载兜底），FlowEditor 抽到 `$lib/components/playground` ——但 G-206 列出 4 项未完（节点 vitest、capability 联动、audit、workflow 持久化） | playground 是"产品脸"，未交付完成 |
| **P2-3** | 前端 | bundle budget 220 KB（G-106）卡在 ChannelTable 30 props，column registry 模式未上 | 首屏体积、长期演进双输 |

---

## 1. 后端架构与性能（详）

### 1.1 🩸 reqwest::Client per-channel — 最大热路径瓶颈

- **证据**：
  - `crates/gate-providers/src/openai.rs:33` `OpenAiProvider::new_with_opts` 内 `reqwest::Client::builder().build()`
  - `crates/gate-providers/src/router/builder.rs` 每个 `build_provider` 走相同模式（Anthropic/Azure/Bedrock/CustomHttp 同理）
  - `ProviderRouter` 持有 `channel_secret_cache`（带 TTL），但 `Provider` 实例本身的复用粒度模糊
- **后果**：
  - 每个 Channel 一份独立 connection pool，跨 channel 不共享 TCP/TLS 连接
  - HTTP2 multiplexing 浪费：同一 host（如 OpenAI 主站）多个 channel 起多个连接
  - 默认 reqwest pool=usize::MAX、idle 90s，但 **builder 不复用** → reload manifest 时旧 client 仍在 GC 前占连接
- **打磨动作**：
  1. 在 `gate-providers/src/lib.rs` 暴露 `SharedHttpClient` —— **全局一个** `reqwest::Client`，按 (connect_timeout, total_timeout) 维度做 small lru（≤ 8）
  2. `OpenAiProvider/AnthropicProvider/...` 改为 `Arc<reqwest::Client>` 注入而非 own
  3. `ProviderRouter::new` 接 `SharedHttpClient`，build_provider 时 clone Arc
  4. 对 plugin（CustomHttpProvider）也走同一池
  5. 验收：`netstat -tn | wc -l` 在 100 channel 共享 OpenAI base_url 时连接数 << 100

### 1.2 🩸 Metrics 几近真空

- **证据**：`rg metrics::|histogram!|counter!|gauge! crates/gate-server crates/gate-providers` 命中 **0**（仅 `worker.rs`、`middleware/quota.rs` 等少量路径有）
- **后果**：
  - chat handler 没 latency / status / channel_id / model 维度的 histogram
  - Provider 上游 error 没分类计数
  - SSE 整流没 frame 计数 / 字节速率
  - Prometheus scrape 拿到的几乎全是 worker 系
- **打磨动作**：
  1. 在 `crates/gate-server/src/metrics.rs` 集中定义 `Names`（`gate_chat_requests_total`, `gate_chat_latency_seconds`, `gate_provider_upstream_errors_total{kind}`, `gate_sse_chunks_total`, `gate_quota_check_total`）
  2. middleware 层做总入口 RED metric（请求数、错误率、p50/p95/p99）—— 把 `middleware/metrics.rs`（31 行，stub 嫌疑）补全
  3. Provider 路径上每次 `chat()`/`chat_stream()` 包一层 helper（在 gate-server 侧，不是 provider 侧——保持 provider 纯上游适配）
  4. 验收：`/metrics` scrape 后 grep `gate_chat_` ≥ 8 个不同 metric

### 1.3 🩸 Provider error 含原始 body 直传

- **证据**：`crates/gate-providers/src/error.rs:42` `Upstream { status, body: String }` —— body 是上游原文，错误日志/审计 sink 可能直接打出
- **后果**：
  - 上游 response 偶尔回显 request body 片段（已知 OpenAI tool_use error 会回显参数）→ 可能含用户 PII
  - `From<reqwest::Error>` 仅按 status code 分支，未保留 retry-after / x-ratelimit-* header
- **打磨动作**：
  1. `body` 改成 `RedactedBody`：保留前 512 字节 + 哈希尾部，避免长 body 撑爆日志
  2. `From<reqwest::Error>` 之外，增加 `from_response(status, headers, body)` 显式构造器，handler 调用时把 retry-after 传进 metadata
  3. 在 `audit_redaction.rs` 配套加 body redaction 规则（已有该模块，扩展即可）

### 1.4 🔥 admin.rs 4235 行 — handler god file

- **证据**：`crates/gate-server/src/routes/admin.rs` 4235 行，一个 `pub fn router()` 内 nest 几十条 `.route()`
- **后果**：编译时间长、PR 冲突高频、关注点混乱（channel CRUD + groups + users + audit + plugin manifest + drain 都在同一文件）
- **打磨动作**（参考 web/src/routes/admin/groups 已经的拆分套路）：
  1. 拆为 `routes/admin/{mod.rs,channels.rs,groups.rs,users.rs,plugin_manifest.rs,audit.rs,drain.rs}`
  2. 每个子模块自己 `pub fn router() -> Router<AppState>`，`admin/mod.rs` 只 `merge`
  3. 验收：`wc -l routes/admin/*.rs` 每文件 ≤ 800

### 1.5 ⚠ Pool 配置未显式

- **证据**：`rg PgPoolOptions|max_connections crates/` 命中 0 直接结果（即 default 配置；推测 `gate-storage` 内部走 sqlx default 10）
- **打磨动作**：在 `gate-storage` 暴露 `PoolConfig { max, min, acquire_timeout, idle_timeout, max_lifetime }`，from `KOOIX_DB_*` env 装配；默认 `max=20, min=2, acquire_timeout=3s, idle=10min, max_lifetime=30min`

### 1.6 ✨ 做得好的地方（保留并扩散）

- **三层 outbox + advisory lock**：`worker.rs` 用 `pg_try_advisory_lock` 实现 multi-replica leader election，无需外部协调器，思路干净
- **inflight pre-debit + sweeper**：流式扣费三段式（pre-debit / settle / refund-on-expire）已经实装（migration `20260519000002` + `worker.rs` `spawn_inflight_sweeper`）
- **fail-open 限流 + Redis 可选**：`middleware/rate_limit.rs` 没 Redis 时不阻断，对自托管友好
- **migration 命名规范**：`YYYYMMDDNNNNNN_topic.sql` 35 个，主题清晰
- **`channel_secret_cache` 带 TTL + invalidate**：`router/mod.rs:206` 做了 secret 解密缓存，避免每次都走 KMS

---

## 2. 渠道实现完整性

### 2.1 Provider 矩阵（实测）

| Provider | LOC | chat | embed | image | audio | stream | tools | reasoning | cache_tokens | 评分 |
|----------|-----|------|-------|-------|-------|--------|-------|-----------|--------------|------|
| openai (`openai.rs`) | 262 | ✓ | ✓ | ✓ | ✓ | ✓ | 透传 | 透传 | 透传 | 6/10（透传≠完整） |
| anthropic (`anthropic.rs`) | 722 | ✓ | ✗ | ✗ | ✗ | ✓ | ✓ map | ⚠ 部分 | ✓ 部分 | 7/10 |
| azure (`azure.rs`) | ? | ✓ | ✓ | ✗ | ✗ | ✓ | 同 OpenAI | 同 | 同 | 6/10 |
| bedrock (`bedrock.rs`) | ? | ✓ | ✓ | ✗ | ✗ | ✓ | ✓ map | ⚠ | ⚠ | 6/10 |
| custom_http (`custom_provider/`) | 4623 | ✓ | ✓ | ✗ | ✗ | ✓ | DSL | DSL | DSL | 8/10（最完整，含 sigv4/sandbox/replay/secrets/fastpath） |

### 2.2 🩸 OpenAI provider 是"裸透传"，不是"适配"

- **证据**：`openai.rs:73` `client.post(...).json(&req).send()` ——`req` 是 `ChatRequest`（gate 自己的类型），直接 serialize 给 OpenAI，参数集是 gate 定义的子集
- **风险**：
  - 用户传 `reasoning_effort=high` 这种新字段，gate 的 `ChatRequest` 如果没字段就掉了
  - 上游加新字段 → gate 类型不更新 → 客户端"看上去能传，实际丢了"
- **打磨动作**：
  1. `ChatRequest` 加 `extra: serde_json::Map<String, Value>` 兜住未识别字段（serde `flatten`）
  2. provider 适配时 merge `req.serialize()` + `extra`，确保前向兼容
  3. 在 docs/provider-coverage.md 列出"已识别字段 vs 透传字段"

### 2.3 🩸 Capabilities 路径不统一

- **证据**：`capabilities.rs` 主要服务 plugin 路径，编译期 4 provider 没暴露 capability 矩阵
- **后果**：playground 想做"capability-aware 节点联动"（G-206）落不下来；前端 ProviderSelect.svelte 只能硬编码
- **打磨动作**：
  1. `Provider` trait 增加 `fn capabilities() -> &'static ProviderCapability`
  2. 4 个编译期 provider 各自写一份 const
  3. 暴露 `/v1/admin/providers/capabilities` endpoint，前端拉取后驱动 UI

### 2.4 🔥 Retry 配置太"全局"

- **证据**：`retry.rs:18` `RetryConfig { max_retries: 2, initial_backoff_ms: 500, ... }`，硬编码 retryable status `[429,500,502,503,504]`
- **缺口**：
  - 没区分 idempotent / non-idempotent（POST chat 流式后失败重试可能造成重复扣费 —— 虽然 inflight 有 sweep，但短窗口仍可能双计）
  - `retryable_error_codes` 是空 vec，从未被填充
  - 没 jitter（exponential backoff 但没随机化，雷暴风险）
- **打磨动作**：
  1. `backoff_ms` 加 ±25% jitter
  2. 流式请求默认 `max_retries=0`（开始流后任何失败都不能 retry，否则 SSE 重发）
  3. 与 inflight pre-debit 联动：retry 前 refund 上次的 pre-debit

### 2.5 🔥 Anthropic cache/reasoning 已写但未挂 Usage

- **证据**：`anthropic.rs` 已 parse `cache_read_input_tokens`，但 `Usage` 结构体（`types.rs`）没有这个字段（`rg cached_tokens|reasoning_tokens crates/gate-providers` 命中 0 → 即虽 anthropic 收到了，没回填到统一 Usage）
- **打磨动作**：
  1. `Usage` 加 `cached_tokens / reasoning_tokens / cache_creation_input_tokens / cache_read_input_tokens`
  2. 各 provider 适配
  3. billing pricing rules 配套加 cache_read 折扣定价（一般 0.1× 正常 input）—— 现在按全价计费 = 直接从客户多收钱

### 2.6 ⚠ Custom Provider sandbox 评估

- **证据**：`custom_provider/` 4623 行，分 7 文件 (`mod / replay / secrets / sandbox / sigv4 / fastpath / helpers`)；`PluginHttpSandbox` + `SandboxDnsResolver`
- **强项**：
  - DSL（render_template / set_path / value_to_*）相对克制，没有 eval JS
  - sigv4 实装了（手写 HMAC + canonical URI / query / headers）—— Bedrock 路径可走 plugin
  - replay 可重放 SSE → 离线测试友好
- **弱项**：
  - DSL 表达力不足时无逃生口（只能升级到 WASM）—— 这是设计选择，OK
  - sandbox 主要管 DNS resolver / endpoint kind，对 path traversal / SSRF 防护需复审

### 2.7 ✨ 做得好

- **HTTP plugin manifest** 是这个项目的**真正护城河**：把 OpenAI/Cohere/Gemini/DeepSeek/Mistral 的"thin wrapper"全退役到 plugin（migration `20260522000001`），1 套 SQL + 1 个 manifest 维护比 5 个 crate 维护干净
- **ADR-0001 / ADR-0003** 路线清晰：编译期 4 个 fast-path + plugin 兜底 + WASM transform
- **plugin_preset.rs** + `adapt_chat_request` 把"大众 provider 都长得像 OpenAI"这件事抽出来 —— 思路对路

---

## 3. WASM 插件实现

### 3.1 实装基线

- **runtime**: wasmtime 26 (`async` feature)
- **ABI**: v0（手写 i32/i64 calling convention，gate_alloc + i64=ptr<<32|len）
- **LOC**: gate-wasm `806` 行（`error / fallback / host / lib / limits / wasmtime_host`），gate-wasm-sdk 较薄
- **Hooks**: `chat_request_transform` / `chat_response_transform` / `stream_chunk_transform`（已通）
- **Limits**: cpu 50ms, memory 16 MiB, no I/O, deterministic — 与 ADR-0003 一致

### 3.2 🩸 host functions 三件套未实装（G-003）

- `host_log` / `host_get_secret_slot` / `host_record_metric` 只在 ADR 列出
- 后果：插件作者写 hello world 想 println 都做不到，DX 直接扑街
- **打磨动作**：
  1. `wasmtime_host.rs` 用 `Linker::func_wrap_async` 绑定三个 host fn
  2. host_log 走 redaction filter（共享 audit_redaction.rs 的规则）
  3. host_get_secret_slot 只接受 manifest 声明的 slot，每次访问写 audit
  4. host_record_metric name 强制 `plugin_wasm_user_` 前缀
  5. examples 加 `wasm-transform-secret-access/`

### 3.3 🩸 编译产物未持久化（G-104）

- 当前每次启动都重新 compile wasm
- **打磨动作**：
  1. 缓存目录 `${XDG_CACHE_HOME}/kooix-gate/wasm/{sha256}-{wasmtime-version}.cwasm`
  2. 启动优先 `Module::deserialize`，失败 fall back compile
  3. validation：deserialize 后跑一次 minimal smoke test 防"陈旧 cwasm"
- **验收**：50 channel × 5 wasm 模块冷启动 < 10s

### 3.4 🔥 模块装配链未自动化（G-002）

- channel manifest 写 `module: "modules/foo.wasm"` 路径字符串，没自动 fetch + instantiate + 挂到 CustomHttpProvider 的 builder 链
- **打磨动作**：抽 `WasmBlobStore` trait（local / s3 / oci），ProviderRouter 构建时迭代 channel.security.wasm 字段做 fetch + instantiate

### 3.5 ⚠ 与 HTTP plugin manifest 关系

- **现状**：HTTP plugin manifest 解决"大部分 provider 长得像 OpenAI"，WASM transform 解决"少数 provider 行为太怪"
- **风险**：两条路线同时演进，schema 重叠度高（auth、retry、capability）—— 已在 plugin_manifest 里通过 `security.wasm` 字段嵌入，方向正确
- **建议**：在 docs/wasm-plugin-abi.md 上增加"决策树"：「先尝试 manifest DSL 解决；只有当 X / Y / Z 时才用 WASM」—— 避免新插件作者直接跳 WASM 抬高成本

### 3.6 ✨ 设计亮点

- **deny-by-default capability**：`allow_fs/allow_net=false` 默认值，host functions 白名单制
- **fallback module**：`fallback.rs` 210 行，`invoke_with_fallback` 模式让 wasm 失败时降级到 raw HTTP，可用性优先
- **wasmtime 26 选型**：是当前 component-model 路径上的稳定 LTS 候选（v0.5.0+ 走 wit-bindgen 不会卡）
- **ABI v0 文档化彻底**：`docs/wasm-plugin-abi.md` 把 calling convention、host fn、resource limits 都写清，业界少见

---

## 4. 前端完整性与产品质量

### 4.1 路由 LOC 分布（已抽组件之后）

| 路由 | LOC | 评估 |
|------|-----|------|
| `channels/+page.svelte` | **1252** | 🩸 god file（已抽 7 个 _components 仍未瘦） |
| `admin/groups/+page.svelte` | 655 | 🟡 0.4.61-64 拆分中（GroupCard/FallbackChainPanel/CanaryComparePanel/BindingTable 已出） |
| `admin/users/+page.svelte` | 650 | 🟡 已抽 4 子组件，仍偏大 |
| `admin/requests/+page.svelte` | 594 | ⚠ 万行 request_logs 必有，需虚拟滚动 |
| `admin/audit/+page.svelte` | 442 | OK |
| `admin/sso/+page.svelte` | 439 | OK |
| `admin/incidents/+page.svelte` | 417 | OK |
| `admin/pricing/+page.svelte` | 370 | OK |
| `dashboard/+page.svelte` | 358 | OK |
| `usage/+page.svelte` | 260 | OK |
| `playground/+page.svelte` | **38** | ✨ 极薄 + dynamic import（FlowEditor 懒加载到 `$lib/components/playground`） |

### 4.2 🩸 channels/+page.svelte 1252 行 god file

- **证据**：import 50+ 个符号、维护 modal/drawer/table 7 个 _components 之间的 state coordination
- **打磨动作**：
  1. 上提 `ChannelTableState` 到 store（`$lib/stores/channels.ts`）—— 当前 state 全埋页面内
  2. modal 协调用 dialog stack pattern：`$lib/components/ui/DialogManager.svelte` 统一打开/关闭/嵌套
  3. API call 抽到 `$lib/api/channels.ts`（fetch + 错误处理 + 乐观更新），页面只 dispatch action
  4. 验收：page.svelte ≤ 400 行

### 4.3 🩸 admin/requests 表格未虚拟化

- 推测大量行数据，需要 virtualization
- **打磨动作**：`templates/DataTable.svelte` 加 `virtualize={true}` 模式（svelte-virtual-list 或自实现 windowing）

### 4.4 🔥 设计系统遵循度（已查）

- emoji UI icon 扫描：**0 命中** ✓
- blue/purple/indigo 装饰色扫描：**0 命中** ✓（`design-classes.ts` 用 zinc + green/amber/red 语义色）
- `controlClass` / `buttonClass` / `cn` token 体系干净
- ⚠ 仍需查：badge variant `admin` 用 `amber-500/20` —— 与 warning amber 区分度不足，建议改 `zinc-500/20` 或加边框区分

### 4.5 🔥 数据加载策略不统一

- 部分页面用 +page.server.ts，部分用 onMount + fetch；缓存策略缺失
- **打磨动作**：
  1. 统一规则写入 `web/src/lib/design/README.md`：「列表页用 +page.ts(load)；操作页 onMount + 重 fetch」
  2. 关键 list query 加 `query` based cache（`@tanstack/svelte-query` 或 sk 自带的 `depends`/`invalidate`）

### 4.6 ⚠ playground G-206 未完

- 未完：7 节点 vitest / capability 矩阵节点联动 / `request_events` audit 接入 / 工作流持久化（`playground_workflows` 表）
- 这是产品脸面，**优先级应高于现在 admin/groups 的进一步拆分**

### 4.7 ✨ 做得好

- **playground 懒加载**：38 行壳 + `dynamic import` `@xyflow` 编辑器，bundle 影响 0
- **templates/ 八件套**（PageShell / AuthFrame / SectionCard / StatePanel / ModalFrame / DataToolbar / FilterPanel / DataTable）覆盖完整
- **lucide-svelte + provider SVG** 双轨，CLAUDE.md 规范严格遵守
- **`npm run check` 0 errors / 0 warnings 是门禁**，TypeScript strict 真打开了

---

## 5. 测试与 CI

### 5.1 数据点

- Rust：`485 (lib 242 + integration 243)` —— 上一次 README 收口确认
- Web: 86 tests
- providers/tests/fixtures 存在，`router/tests.rs` `plugin_manifest/tests.rs` 在 crate 内

### 5.2 缺口

- **chaos test 缺**：限流挂掉 / Redis 闪断 / 上游 503 风暴 / pool 耗尽 —— 没有 deterministic 复现 case
- **bench 只在 `crates/gate-providers/benches`、`crates/gate-wasm/benches`**，但**没 server 整链 bench**（chat e2e p99）
- **golden file 测 SSE normalize**：可以（已有 fixtures），但**多 provider 的 SSE event 比对覆盖率**未文档化

### 5.3 打磨动作

1. 加 `crates/gate-server/benches/chat_e2e.rs`（mock 上游 + criterion，量 chat handler 内部各 stage 耗时）
2. `tests/chaos/`（用 testcontainers + toxiproxy 注入 Redis/PG 故障）
3. SSE conformance suite：每个 provider 一份 expected golden（OpenAI / Anthropic / Bedrock / 3 个 plugin preset），`cargo test --test sse_conformance`

---

## 6. 安全收口

### 6.1 已做

- ✅ Master Key + envelope KMS（`gate-crypto`）
- ✅ AAD 绑定（DESIGN.md § 7.2）
- ✅ JWT secret + previous secrets 轮换
- ✅ `audit_redaction.rs` 已存在
- ✅ require! 纪律（DESIGN § 7.4）
- ✅ Row-Level Security（`middleware/rls.rs`）

### 6.2 ⚠ 待打磨

- **Provider error body redaction**：见 §1.3
- **plugin manifest signature**：`docs/manifest-registry-signature.md` 有设计，落地状态需复核（建议下版本前过 `cargo run -p kgctl -- manifest verify` 走通）
- **审计完整性证明**：当前 audit log 是单向写入；建议加每日 hash chain（每 1000 条算 SHA256 入 ledger，月度对账）—— 合规友好
- **CTF/抽样审一次 /v1/chat/completions auth**：API key 校验路径建议加 timing-safe compare（`subtle::ConstantTimeEq`），防 timing attack

---

## 7. 优先级建议（按"产品价值/成本比"排序）

### 第一刀（一周内可落地，单 PR 闭环）

| # | 动作 | 文件 | 预计 LOC | 收益 |
|---|------|------|----------|------|
| A1 | SharedHttpClient 抽离 | `gate-providers/src/lib.rs` + 4 provider | +200 / -50 | 高并发连接复用、TLS 复用、扩 100 channel 不爆连接 |
| A2 | gate_chat_* metric 套件 | `gate-server/src/metrics.rs` + chat/embeddings/responses | +300 | 上线后能观测、SLO 立得起来 |
| A3 | ChatRequest extra flatten | `types.rs` + 4 provider 序列化 | +50 | 上游加字段不漂移 |
| A4 | Usage 加 cached/reasoning tokens | `types.rs` + anthropic / bedrock | +100 | billing 准确性，stop overcharging |
| A5 | provider error body redaction | `error.rs` + audit_redaction | +80 | 日志/审计安全收口 |

### 第二刀（两周内）

| # | 动作 | 收益 |
|---|------|------|
| B1 | admin.rs 拆分 7 子文件 | 维护性、PR 冲突减半 |
| B2 | channels/+page.svelte 拆 store + dialog manager | 前端核心页面交付级 |
| B3 | WASM host functions 三件套 + cwasm 持久化 | 插件 DX 起步、冷启动 |
| B4 | DataTable virtualize for admin/requests | 万行可滚 |
| B5 | provider capabilities 暴露 | playground 联动落地 |

### 第三刀（一个月内）

- Playground G-206 全部 4 项收口 → 把 playground 提升到产品级 demo
- SCIM v2（G-105）+ SAML（G-202）补企业能力
- chaos test + e2e bench 起步
- DB pool 配置显式化 + buffered request_log writer

---

## 8. 一行结论

> 项目骨架硬、设计 ADR 齐、ROADMAP 收敛清楚——但**"骨架完整 ≠ 产品完整"**。
>
> 离"优秀产品"差的两口劲：**热路径性能化**（SharedClient + metrics）+ **门面打磨**（channels god page + playground 收口 + WASM host fn）。
>
> 干完上面"第一刀 + 第二刀"，可以打 v0.5.0-rc1，对外宣称"production-ready open-source LLM gateway"。

---

*Reviewer: 邪修红尘仙 / Date: 2026-05-26 / Baseline: v0.4.64*
