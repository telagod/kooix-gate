# Product Gaps — Kooix Gate v0.4.60 → v0.5.0

> Status: **0.4.60 完整产品形态宣告后的对账清单**（2026-05-23）
> 来源：代码 + 文档 + ROADMAP § M4 联合比对
> 用途：v0.5.0 启动会议据此筛选、切片、定时间盒

## 阅读说明

每条 gap 标三类元信息：

- **影响面**：runtime / security / sdk / ops / ux / billing / enterprise
- **当前状态**：✅ 已落 placeholder / schema / setter；🟡 仅设计稿；⛔ 完全未起
- **0.5.0 含义**：v0.5.0 启动后是否进入第一波

> 没有列入此文档的功能默认走 [ROADMAP.md](../ROADMAP.md) 已完成基线，不再单独追踪。

---

## 已收口（0.4.65-0.4.117，product-review 双刀）

> 关联：[product-review-2026-05-26.md](./product-review-2026-05-26.md) 第一刀 + [product-review-followup-2026-05-26.md](./product-review-followup-2026-05-26.md) 第二刀（自我批判）。
> 总计 53 个 patch（0.4.65-0.4.117），主线 main 已合入。

### 第一刀（0.4.65-0.4.101，37 patch）

| 项 | 版本 | 内容 | 验证 |
|----|------|------|------|
| A1 | 0.4.65 | SharedHttpClient — 4 fast-path provider 共享 reqwest pool | 124 providers tests |
| A2 | 0.4.66 | `gate_chat_*` 4 个 metric（duration / ttfb / stream_chunks / requests_total） | 45 server tests |
| A3 | 0.4.67 | Anthropic / Bedrock 转译型 provider 透传 ChatRequest.extra | 127 providers tests |
| A4 | 0.4.68 | Usage 增 `cache_creation_input_tokens` + OpenAI o1/o3 details 自动 lift | 130 providers tests |
| A5 | 0.4.69 | ProviderError body 脱敏（512B 截断 + sha256 哈希尾） | 134 providers tests |
| —  | 0.4.70 | Retry ±25% jitter + `RetryConfig::stream_safe()` | 139 providers tests |
| —  | 0.4.71 | PgPool 配置显式化（`KOOIX_DB_*` env，5 字段） | 5 storage tests |
| —  | 0.4.72 | admin.rs B1 step 1/4: pricing 块封装内联 mod | 45 server tests |
| —  | 0.4.80-82 | WASM host_log + host_record_metric 实装（G-003 step 1+2/3） | 18 wasm tests |
| —  | 0.4.83-84 | WASM cwasm 持久化缓存 + KOOIX_WASM_CACHE_DIR | 18 wasm tests |
| —  | 0.4.87-88 | `GET /v1/admin/providers/capabilities` endpoint + test | 46 server tests |
| —  | 0.4.101 | 「空衍」logo 重设计（D4 → 风车螺旋） | npm check 0/0 |

### 第二刀（0.4.102-0.4.117，16 patch · 自我批判修正）

| 项 | 版本 | 类型 | 内容 |
|----|------|------|------|
| — | 0.4.102 | docs | followup 批判稿（揭第一刀 6 类粉饰） |
| §3.1 | 0.4.103 | runtime | Retry-After 兼容 HTTP-date（RFC 7231） |
| §3.2 | 0.4.104 | runtime | Usage 加 audio_tokens / accepted+rejected_prediction_tokens |
| §3.3 | 0.4.105 | runtime | SharedClient LRU per-key eviction 防雷暴 |
| §3.4 | 0.4.106 | test | chat handler 埋点 grep 验证 4 个 callsite |
| §3.5 | 0.4.107 | refactor | metric 名抽 `pub mod names` const |
| §6 | 0.4.108 | docs | stream_safe 语义钉死注释 |
| §4 | 0.4.109 | refactor | admin.rs org_members 块抽 inline mod |
| §1 | 0.4.110 | refactor | channels page form-factories 抽 _lib |
| §1 | 0.4.111 | design | host_get_secret_slot 完整 ABI 设计稿 |
| §5.1 | 0.4.112 | docs | Grafana dashboard 修指标名漂移 + 加 4 panel |
| — | 0.4.113 | docs | 撤回 §1 P1-3 误判（request_logs 已 outbox 异步）|
| — | 0.4.114 | test | channels form-factories 4 sanity tests |
| §1 | 0.4.115 | design | DataTable virtualization 完整设计稿 |
| §4 | 0.4.116 | docs | admin.rs 拆分进度表 + ROADMAP 第二刀汇总 |
| §5.2 | 0.4.117 | docs | SECURITY.md 完整化（SLA / disclosure / severity tiers） |

### 第二刀诚实评

- **真改 runtime 5 项**：Retry-After / Usage 字段 / SharedClient LRU / metric const / org_members 内联
- **设计稿 / 文档化 / 测试 11 项**：补真实方案图纸或锁业务契约
- **撤回 1 项误判**：request_logs 实际已是 outbox 异步路径
- **粉饰更正**：第一刀的 "step 1/N" 命名 + "占位 env" + "幽灵 API" 全部摊到 followup 文档面前

### 真实债务推到 v0.5.x

参见 [ROADMAP.md § M3 后 — product-review 第二刀 · 剩余真重构](../ROADMAP.md)：

- admin.rs 5 大块物理拆分（channels / users / sso / groups / invitations / probe）
- channels page B2 step 3-4（list state store + dialog manager）
- DataTable virtualize 实装（按 0.4.115 设计稿）
- host_get_secret_slot 实装（按 0.4.111 设计稿）
- WASM module blob store + auto-mount (G-002)
- chat e2e bench / chaos test runtime
- playground frontend capability 联动

---

## P0 — 信任链与运行时收口（v0.5.0 必交付）

### G-001 真实公钥验签链

- 影响面：security / supply chain
- 当前状态：✅ schema（[manifest-registry-signature.md](./manifest-registry-signature.md) typed `kind/value/key_id/alg`）+ 格式校验（`crates/kgctl/src/plugin.rs verify_minisign_format` 仅 base64 + length ≥ 64）
- 缺口：cosign / sigstore-rs / minisign 真正调用公钥验签，目前是 placeholder
- 实施路径：
  - 引入 `sigstore-rs` 或 `cosign-rs` 处理 sigstore_bundle / cosign keyless
  - 引入 `minisign-verify` 或自实现 ed25519-dalek 验签 minisign payload
  - `kgctl plugin registry verify` 默认 strict mode，`--allow-unsigned` 才放行
- 验收门禁：恶意 manifest 修改后 sha256 / signature 任一不匹配，registry import 必须拒绝并 audit
- 关联：ADR-0003 § Trust chain；0.4.54 已落 schema 槽位

### G-002 WASM 模块外部存储 + auto-mount

- 影响面：runtime / ops
- 当前状态：✅ `ProviderRouter::with_wasm_host` / `wasm_host()` setter+getter（0.4.57）；✅ `WasmtimeHost::load_module` 接受字节流
- 缺口：channel manifest 里只有 `module: "modules/foo.wasm"` 路径字符串，没有「自动按 manifest 拉字节 → instantiate → 挂到 CustomHttpProvider」的 builder 装配链
- 实施路径：
  - 抽象 `WasmBlobStore` trait（local fs / S3 / OCI artifact）
  - `ProviderRouter` 构建时迭代 channel.security.wasm 字段，按 `module_sha256` 命中 cache，未命中走 blob store fetch
  - `WasmtimeHost` 编译产物缓存到 disk（`Module::serialize` + 启动时 `Module::deserialize`），冷启动 ~ms 级
- 验收门禁：50 channel × 5 个独立 wasm 模块，gate-server 冷启动 < 10s

### G-003 真实 host functions 暴露

- 影响面：runtime / sdk
- 当前状态：🟡 ABI v0 三个 transform hook 已通；`host_log` / `host_get_secret_slot` / `host_record_metric` 仅 ADR-0003 § v0 host functions 列出，未实装
- 缺口：插件无法记录 log / 拿 secret slot / 上报自定义 metric
- 实施路径：
  - `wasmtime::Linker::func_wrap_async` 绑定 host_log（带 redaction 过滤）
  - host_get_secret_slot 只接受 manifest 声明过的 slot，每次访问写 audit `plugin.wasm.secret_access`
  - host_record_metric 走 `metrics::counter!` / `histogram!`，name 必须以 `plugin_wasm_user_` 前缀，防 namespace 污染
- 验收门禁：例子 `examples/wasm-transform-secret-access/` 能跑通 + audit log 落地

### G-004 完整 stream_chunk_transform 双向解码

- 影响面：runtime
- 当前状态：✅ 0.4.51-0.4.52 已接通 `stream_chunk_transform` + wiremock SSE e2e
- 缺口：当前 chunk 是「按字节切片穿透」；尚未把 SSE event 分帧后再喂给 wasm（host 解码 SSE → 逐 event 调 wasm → 再编回 SSE）
- 实施路径：
  - host 侧的 SSE decoder（`gate-providers/src/sse_normalizer`）暴露 frame 接口
  - wasm hook 改为 `stream_event_transform(event: SseEvent) -> SseEvent`
  - 兼容性：保留 `stream_chunk_transform` 作为 raw bytes 兜底
- 关联：[wasm-plugin-abi.md § Streaming transform](./wasm-plugin-abi.md)

---

## P1 — DX 与生态铺路（v0.5.0 推荐交付）

### G-101 AssemblyScript SDK npm 发布

- 影响面：sdk / dx
- 当前状态：✅ 本地 package（`sdks/gate-wasm-sdk-as/` v0.4.55-0.4.56）`@kooix-gate/wasm-sdk-as`
- 缺口：未发布到 npm registry，外部用户拿不到
- 实施路径：
  - GitHub Actions `release-as-sdk.yml`：tag 打 `as-sdk-vX.Y.Z` 触发 `npm publish --access public`
  - npm trusted publisher（OIDC，不存 NPM_TOKEN）
- 验收：`npm i @kooix-gate/wasm-sdk-as` 可用

### G-102 管理面 WASM 表单 UI

- 影响面：ux / runtime
- 当前状态：⛔ 当前 channels drawer 没有专门的 wasm 模块字段；用户需手填 `model_mapping.plugin.security.wasm` JSON
- 缺口：drawer 内独立 section：模块上传 / sha256 自动算 / hook 多选 / 资源限制 form
- 实施路径：
  - 新组件 `web/src/routes/channels/_components/WasmModulePanel.svelte`
  - 上传走 `POST /v1/admin/wasm-modules`（新 endpoint），返回 sha256 + size
  - 模块字节存到 `wasm_modules` 表或 BlobStore（与 G-002 联动）
- 关联：G-002

### G-103 WASM ABI v1 走 wit-bindgen

- 影响面：sdk / runtime
- 当前状态：✅ ABI v0 手写 i32/i64 calling convention（`gate_alloc` + `i64 = ptr<<32 | len`）
- 缺口：v0 维护成本高，跨语言 SDK 需各自实现 ABI；component-model 是业界方向
- 实施路径：
  - 定义 `wit/kooix-plugin.wit`（[wasm-plugin-abi.md § WIT 草案](./wasm-plugin-abi.md) 已有蓝图）
  - wasmtime 26 已支持 component-model
  - 提供 v0 → v1 双跑窗口，至少一个 minor 版本
- 关联：[wasm-plugin-abi.md § ABI 版本与 WIT 草案](./wasm-plugin-abi.md)

### G-104 WASM 编译产物持久化缓存

- 影响面：runtime / perf
- 当前状态：⛔ 当前每次 gate-server 启动都重新编译 wasm 模块
- 缺口：模块多/大时冷启动慢；社区方案 `Module::serialize` + 启动时 `Module::deserialize`
- 实施路径：
  - 缓存目录默认 `${XDG_CACHE_HOME:-~/.cache}/kooix-gate/wasm/`
  - 文件名 `{module_sha256}-{wasmtime_version}.cwasm`
  - 启动时优先 deserialize；deserialize 失败 fall back 到 compile
- 关联：G-002

### G-105 SCIM v2 实装

- 影响面：enterprise / identity
- 当前状态：✅ [scim-evaluation.md](./scim-evaluation.md) 评估完成
- 缺口：endpoint 未实装；group → role mapping 未支持
- 实施路径：
  - 新 routes `/scim/v2/Users` / `Groups` 在 `gate-server`
  - 鉴权走 Bearer + SCIM-specific role
  - 用户 dedup：`externalId` binding，email + status 同步
  - group mapping 由管理员显式配置到 Org / Project role
- 关联：scim-evaluation.md 已定边界

### G-106 Web bundle 220 → 180 KB

- 影响面：ux / perf
- 当前状态：✅ 0.4.18 budget 250 → 220KB；CI 集成 `bundle:budget` 门禁
- 缺口：channels 页 ChannelTable 段约 30 props 未硬拆，是 220 → 180 的卡点
- 实施路径：
  - ChannelTable column registry 模式：每列独立 props
  - DataTable virtualization for 大表
  - 复用 `lib/design/classes.ts` token 收敛重复 class

---

## P2 — 企业 / SaaS 进阶（v0.5.x 后续筛选）

### G-201 SaaS 多区域路由

- 影响面：enterprise / routing
- 当前状态：⛔ 完全未起
- 缺口：跨 region failover、data sovereignty、按 latency 分区
- 实施路径：
  - Channel 增加 `region` 字段（multi-value）
  - Routing strategy 增 `geo_proximity`，按 client IP / org default region 优先
  - 数据流转策略：可声明「禁止跨 EU 边界 settle」
- 验收：org 配置 `region=eu-west` 后，channel `region=us-east` 不进入路由候选

### G-202 SAML 2.0 SSO

- 影响面：enterprise / identity
- 当前状态：✅ OIDC / SSO Provider 完整（0.4.x 已 GA）
- 缺口：传统企业仍要 SAML；可选项
- 实施路径：`samael` crate 或自实现；与 OIDC 共用 IdentityProvider 表

### G-203 OpenTelemetry log export

- 影响面：ops
- 当前状态：✅ OTLP trace + Prometheus metric
- 缺口：log 仍仅 `tracing_subscriber` stdout，未接 OTLP log
- 实施路径：`opentelemetry-appender-tracing`，gate 顶层 init 时按 `KOOIX_OTLP_LOGS_ENABLED` 切换

### G-204 Cost forecasting / 预算预测

- 影响面：billing / ux
- 当前状态：✅ 月账单 + 预算 50/80/100% 告警
- 缺口：基于历史 usage_records 做下月预测，配合 budget 提前预警
- 实施路径：worker plane 跑日级 rollup，前端可视化曲线 + 简单 EWMA / Holt-Winters

### G-205 Stripe 等支付 gateway

- 影响面：billing / saas
- 当前状态：✅ Invoice 状态机（draft → closed → exported → paid/waived）
- 缺口：`exported → paid` 当前靠人工标记；接 Stripe webhook 可自动闭环
- 实施路径：`stripe-rs` SDK；webhook 走 `gate-server` 独立 route + HMAC 验证

### G-206 Playground 收尾（M1.5 残留）

- 影响面：ux / product line
- 当前状态：M1.5 收编完成，仍有 4 项 TODO（[ROADMAP § M1.5](../ROADMAP.md#m15-playground-收编为产品线)）
- 缺口：
  - 7 种节点 vitest 覆盖
  - Capability 矩阵节点联动
  - `request_events` audit 接入
  - 工作流持久化（`playground_workflows` 表）
- 实施路径：按节点逐个补 vitest；capability 矩阵复用现有 ProviderCapability；workflow 表与 audit 流水复用现有 outbox

---

## P3 — 长期愿景（不进 v0.5.0，留待 v0.6.0+）

### G-301 多 host language SDK（Go / Python / Zig）

- 影响面：sdk / ecosystem
- 触发条件：G-103 ABI v1 通过 wit-bindgen 跑通后，再扩 Go / Python / Zig

### G-302 WASM 跨 instance 共享 module cache

- 影响面：runtime / perf
- 触发条件：单实例模块数 > 50 后再做

### G-303 完整 fine-grained ABAC

- 影响面：security / policy
- 当前 RBAC + scope 足够 80% 场景；ABAC 留到企业明确需求

---

## 验收对账

每条 gap 关闭时必须：

1. CHANGELOG 写明对应 G-编号
2. 本文档对应条目状态从 ⛔ / 🟡 / ✅ schema 升级为 ✅ closed (vX.Y.Z)
3. 关联文档（ADR / runbook / api-reference）同步

## 引用

- [ROADMAP.md § M4](../ROADMAP.md#m4--v050--enterprise--saas-进阶候选)
- [ADR-0003 v0](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [wasm-plugin-abi.md](./wasm-plugin-abi.md)
- [manifest-registry-signature.md](./manifest-registry-signature.md)
- [scim-evaluation.md](./scim-evaluation.md)
- [CHANGELOG.md § 0.5.0 真候选](../CHANGELOG.md)
