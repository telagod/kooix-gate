# Changelog

All notable changes to **Kooix Gate** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added — Plugin Manifest v1

- 新增 HTTP Plugin manifest v1 强类型解析，固定 `metadata` / `capabilities` / `auth` / `request` / `response` / `stream` / `usage` / `error` / `probe` / `security` 顶层分区。
- 保留 v0 manifest 自动升级路径；`model_mapping.plugin` 仍是存储入口，但运行期会解析为 v1 内部结构并返回 JSON pointer 错误。
- 新增 `GET /v1/admin/plugin-manifest/schema` 与 `kgctl plugin schema|lint`，让后端校验、CLI lint 和后续前端表单共用同一 JSON Schema。
- Plugin runtime 开始按 `auth.strategy` 注入认证：`bearer` / `api_key_header` / `api_key_query` / `basic` / `custom_headers` / `hmac` / `aws_sigv4` / `oauth_client_credentials` / `none`，preset 会自动映射 Azure `api-key`、Anthropic `x-api-key` 与 Bedrock SigV4。
- Plugin secret slots 统一接入 `channel_keys.label`：同一 channel 的 active encrypted keys 会解密为 slot map，manifest 只引用 `secret_slot` / `username_slot` / `password_slot`，运行时不接受明文 secret。
- Plugin manifest 新增 `hmac` auth strategy：按 method/path/query/body_sha256/timestamp/nonce 生成 HMAC-SHA256 签名，并自动注入 timestamp、nonce 与 signature header。
- Plugin manifest 新增 `aws_sigv4` auth strategy，并把 Bedrock Converse preset 切到正式 AWS Signature Version 4；不再注入临时 `X-Amz-Access-Key` / `X-Amz-Secret-Key` header。
- Plugin manifest 新增 `oauth_client_credentials` auth strategy：用 `client_id_slot` / `client_secret_slot` 向 `token_url` 换取 access token，运行时缓存 token 并按过期时间刷新，再注入 `Authorization: Bearer <token>`。
- Channel 创建 / 编辑抽出 Plugin Auth Strategy 表单，会按 `bearer` / `api_key_header` / `api_key_query` / `basic` / `custom_headers` / `hmac` / `aws_sigv4` / `oauth_client_credentials` / `none` 展示最小字段，并在保存前把 auth 合并进 manifest 做本地 lint。
- Plugin request mapping DSL 扩展到 `tools` / `tool_choice` / `metadata.*` / `extra.*`，整段占位继续保留 JSON 原类型；path、query、header、body 中缺失或空值的条件字段会自动跳过，避免私有上游拒绝未知空字段。
- Plugin channel 的 `model_mapping` 可同时保留 `plugin` manifest 与 `models` / `model_aliases` / `deployments` 映射，让 model alias、Azure/Bedrock preset 与私有 deployment path 都通过 manifest 链路改写。
- Plugin response / usage 映射升级为稳定 path evaluator：支持 nested object、array index、`|` first non-null fallback 与 `default:` literal；非流式 response 可声明 `reasoning_content_path`、`tool_calls_path`、`request_id_path`、`metadata_path`，usage 可抽取 reasoning tokens、image units、audio seconds 与 vendor raw usage。
- `ChatResponse` 保留上游 request id / metadata，`Usage` 保留 raw usage 与多模态用量；pricing 管理页维度与后端 `pricing_rules` 命名对齐，避免 `images_generated` / `audio_seconds_in` 等旧维度写入后无法被计费引擎消费。
- SSE normalizer 产品化：`stream.ignore_events` / `done_events` 支持 `event:` 分流，`done_path` / `done_values` 支持 vendor done object，`tool_calls_path` 支持私有 tool call delta，usage-only 末帧可按 raw / reasoning / cached 等维度触发输出。
- 新增 SSE replay harness：`POST /v1/admin/plugin-manifest/replay`、`kgctl plugin replay` 与 Channel UI `SSE replay preview` 均可用同一 manifest 回放 raw SSE 并预览 OpenAI-compatible chunks。
- 流式计费门禁改为 fail-closed：上游缺失 usage 末帧时按 request message / `max_tokens` 生成 estimated usage，写入 outbox 并以 `raw.estimated=true` 标记，不再静默跳过 billing / quota settlement。
- Plugin error mapper 开始消费 `error.status_path` / `code_path` / `message_path`，把上游 auth、rate limit、model missing、vendor safety block 与未知 5xx 分别归一为 `authentication_error`、`rate_limit_error`、`invalid_request_error`、`policy_error` 与 retryable upstream error。
- Plugin `request.retry` / `error` 可声明 retryable status/code、cooldown 与 circuit breaker 阈值；chat runtime 会把失败写入 `channel_keys` 统计，按 manifest 阈值进入 `cooling_down`，路由自动跳过冷却 key/channel 并落 `upstream_errors_total` 观测指标。
- Plugin `probe` 可声明轻量模型、probe path/body、成功状态码与 `max_cost_micros`；后台 health checker 与 `POST /v1/admin/channels/:id/probe` 均按 manifest 发起探活，成功会恢复 channel 并同步模型，失败会进入原有 health/fallback 链路。
- Manifest Builder / Debugger 补齐：Channel 创建抽屉新增 7 步 builder（preset/auth/request/response sample/SSE replay/test/save+group），response sample 可点选生成 path mapping，保存后可自动加入 channel group。
- `kgctl plugin test|export|import` 落地；`export` 生成包含 manifest、response sample、raw SSE 与 expected chunks 的 golden fixture，`import --verify` 可在 schema / normalizer 升级后回放验证。
- Provider capability matrix 落地：编译期 Provider 与 runtime plugin preset 共享 `ProviderCapabilities`（chat / streaming / tools / embeddings / image / audio / vision / json_mode / batch），Admin Channel / Group binding API 返回 capability，chat route 会按 stream/tools/vision/JSON mode 跳过不满足能力的 channel。
- Provider preset 增加 capability 默认值与 Base URL 建议；OpenAI-compatible 变体补齐 `vllm`、`lm_studio`、`ollama_openai`、`localai`、`xinference`。
- Channel 控制台新增 capability chips、Base URL 建议与不可用能力提示；创建/编辑 plugin preset 时 manifest 自动写入完整 capability 默认值。
- `/v1/models` 现在只聚合 active + healthy channel，并在每个 model 上返回所有可用 channel capability 的 union，帮助 OpenAI-compatible 客户端在迁移前判断 streaming/tools/embeddings/image/audio/vision/json mode 能力。
- `/v1/embeddings` 现在走 ProviderRouter 的 embedding channel 路由，贯通 model alias / channel model mapping、`channel_id`、channel key success/failure 上报与 least_conn inflight release。
- `/v1/embeddings` 成功响应会按 upstream `usage` 写入 billing outbox；consumer 落库后可在 `usage_records`、`request_events` 与 request log read model 中对账，`completion_tokens` 固定为 0。
- `/v1/images/generations` 接入 ProviderRouter image channel 路由，按 capability `image=true` 选择 OpenAI-compatible image runtime，并贯通 model mapping、`channel_id`、channel key health 与 least_conn release。
- `/v1/images/generations` 成功响应会按 billable image units 写入 billing outbox，支持 `per_image` pricing conditions（`quality` / `size`），consumer 落库后可进入 `usage_records`、`request_events` 与 request log read model。
- `/v1/audio/speech` 与 `/v1/audio/transcriptions` 接入 ProviderRouter audio channel 路由，按 capability `audio=true` 选择 OpenAI-compatible audio runtime，并贯通 model mapping、`channel_id`、channel key health 与 least_conn release。
- `/v1/audio/speech` 成功响应按 `tts_characters` 写入 billing outbox，可命中 `per_character_tts` pricing；`/v1/audio/transcriptions` 初版按 `per_request` 计费，并在 raw usage 中保留 filename / language / audio bytes。
- `/v1/responses` 落地 thin adapter：把 Responses API 的 `input` / `instructions` / `stream` / `tools` / `tool_choice` / `max_output_tokens` 映射到 chat pipeline，复用现有路由、provider、billing、quota 与 request-id 链路。

### Changed — Error Shape

- Data-plane error shape 统一为 `{ error: { code, type, message, ... } }`：上游 auth → `authentication_error`，上游 rate limit → `rate_limit_error` + `Retry-After`，quota → `quota_exceeded` / `quota_error`，model miss → `model_not_found`，no healthy route → `no_healthy_channel`。
- OpenAI-compatible、Anthropic、Bedrock 与 HTTP Plugin error mapper 均把上游 404 / model missing 归一为 `ProviderError::ModelNotFound` / `NormalizedProviderErrorKind::ModelNotFound`，避免继续落到泛化 `invalid_request_error` 或 `upstream_error`。
- chat/embeddings/images/audio 的 channel key failure policy 改为共用 `provider_failure_policy`，health cooldown、circuit breaker error code 与 `upstream_errors_total` 统一口径。

### Changed — Routing / Health

- Health checker 标准化 compile-time provider probe：按 provider 默认低成本模型构造 `/models` 或最小 chat probe，统一声明 `max_cost_micros=25`，并保留 channel `supported_models` 优先覆盖默认模型。
- 后台 health probe 现在写入 `provider_health_probe_total` 与 `provider_health_probe_duration_seconds`，使用 bounded `provider_type/outcome/status_bucket` 标签，覆盖成功率、延迟与错误码分桶。
- Health checker 会把 probe 成功/失败与延迟喂回 `ProviderRouter` 的 `ChannelMetrics`，让 `least_latency` 在无真实请求热度时也有健康巡检样本。
- `least_latency` 从单进程内存均值升级为 `channel_latency_samples` 持久化滑窗：chat / responses 请求与 health probe 都写入 `request|health_probe` 低基数字段，路由热路径按候选 channel 一次批量查询窗口均值，DB 异常时 fail-open 回退内存 `ChannelMetrics`。
- Channel Group detail API 增加 `fallback_chain` 与 `fallback_stats`，按 `request_events.group_id` 统计近 24h primary / fallback 请求量、fallback hit-rate 与链路节点占比。
- Channel Group 创建 / 更新会校验 `fallback_group_id` 存在、禁止自引用、禁止循环并限制最大深度 5；控制台回退候选同步过滤会成环的分组。
- 控制台 `/admin/groups` 增加 fallback chain 图、节点请求占比、fallback 命中率与循环告警；create modal 的 `description` / `fallback_group_id` 现在由后端真实持久化。
- billing usage event 增加可选 `group_id`，chat / responses / embeddings / images / audio 路由命中后写入 `request_events` 与 `usage_records`，作为 fallback 命中率和后续 group 维度对账来源。
- Channel 新增 `draining` 状态与运维 API：`POST /v1/admin/channels/:id/drain` 禁止新请求，`GET /drain-status` 返回当前 router inflight，`POST /disable-when-idle` 仅在 inflight 清空后禁用 channel。
- 控制台 Channel 列表与详情页增加 Drain / 空闲禁用入口、Draining badge、inflight 刷新与安全下线提示；`/admin/channels` 仪表盘同步统计 Draining 渠道数。
- Channel Group binding 新增 `canary_percent_bps`：控制面限制 1%-5% canary 流量，路由热路径用 deterministic gate 跳过未命中 canary binding，避免把权重误当灰度比例。
- Channel Group detail API 与控制台新增 `canary_stats`，按近 24h `request_events` 自动比较 canary / baseline 的请求量、错误率、平均延迟与平均成本。

### Added — Billing / Ledger

- `billing_ledger_events` 补齐显式 `event_type`：`estimated_debit` / `actual_settle` / `refund` / `manual_adjustment` / `invoice_close`，并增加 `invoice_month` 与 org-level adjustment / invoice close 所需的 nullable project/api_key 支持。
- `gate-billing` 新增 typed ledger event constructors 与 `reconcile_usage_ledger` 对账任务，能按窗口比较 `usage_records` 与 `actual_settle` ledger 的缺失、孤儿与金额差异。
- 月账单聚合优先从 `billing_ledger_events.actual_settle` 重建费用，`usage_records` 退为 tokens/model/project analytics projection。
- 新增 `billing_invoices` 月账单状态机：`draft -> closed -> exported -> paid/waived`，控制面提供 `POST /v1/orgs/:org_id/billing/:month/state` 推进状态并写 audit。
- Billing CSV 导出增加 `x-kooix-export-digest=sha256:<hex>`；新增 `/v1/orgs/:org_id/billing/export.json`，响应内嵌 rows 与 digest 便于审计留存。
- Pricing 控制台新增 Conditions JSON editor 与常见模板：cache、image size、audio seconds、batch、region。
- 成本告警扩展为预算 50/80/100% 阈值，并保留 pricing miss 与高成本异常的可观测入口。

### Fixed — Quota / Billing

- 修复 budget quota pre-debit 的 `inflight_requests` 写入竞态：中间件不再后台 spawn insert，避免 handler 先 settle/delete 后 insert 才落库，导致同一 `x-request-id` 的 inflight 行残留并破坏 crash recovery 对账。
- budget quota pre-debit 支持解析 `EmbeddingRequest`，按 embedding input 字符数估算预扣，并在 `/v1/embeddings` 完成后用实际 `usage.total_tokens` settle / refund。
- `/v1/embeddings` 上游失败不再包装成 internal error；auth、rate limit、invalid request、policy、network、decode 与 mapped error 进入统一 provider error shape，并同步 channel key cooldown / circuit breaker 统计。
- budget quota pre-debit 支持解析 `ImageGenerationRequest`，按默认 `$0.08/image` 估算 image 请求预扣，并在 `/v1/images/generations` 完成后按 billable image units settle。
- `/v1/images/generations` 上游失败不再包装成 internal error，统一进入 provider error shape 与 channel key failure 统计。
- budget quota pre-debit 支持解析 `AudioSpeechRequest`，按 TTS input 字符数估算预扣，并在 `/v1/audio/speech` 完成后按 `tts_characters` settle。
- `/v1/audio/speech` 与 `/v1/audio/transcriptions` 上游失败不再包装成 internal error，统一进入 provider error shape 与 channel key failure 统计。

### Changed — Docs

- 整理文档入口：新增 `docs/README.md` 与 `docs/stages/README.md`，把已完成的重构审计记录归入 `docs/stages/`，保留 active waivers 原路径供 CI / quality gate 使用。
- 新增阶段性记录 `docs/stages/2026-05-19-docs-and-secret-scan.md`，把文档分层清理、gitleaks 本机安装复验与本轮 Plugin secret slot 验证证据归档，根目录保持干净。
- `kgctl doctor --json` 输出 `{ ok, checks[] }` 机器可读体检报告；失败仍保持非零退出码，供 CI / deploy pipeline 消费。
- `kgctl smoke` 增加发布后 HTTP E2E：登录、创建 smoke project/channel/group/API key、发送 `/v1/chat/completions`、查询 `/v1/usage`。
- 新增 `examples/`：OpenAI SDK、curl streaming、Provider preset channel、HTTP Plugin manifest、private auth/field mapping/SSE、pricing、quota、OpenAPI、Postman、Bruno、Terraform、Helm 示例。

## [0.2.0] — 2026-05-18

第一个正式发布版本。相比 v0.1.5，本版把 typed ID、定价规则 CRUD、crash-safe quota pre-debit、HTTP Plugin 归一化、Provider 插件预设、前端模板化与发布边界一起收口。

### Added — API / Admin / CLI

- API response 统一返回带前缀 typed ID（如 `org_...` / `proj_...` / `usr_...`）；URL path 参数通过 `FlexUuid` 同时接受 typed ID 与裸 UUID。
- 定价规则 CRUD 补齐到三条入口：
  - REST：`GET/POST /v1/admin/pricing-rules`、`DELETE /v1/admin/pricing-rules/:id`
  - CLI：`kgctl pricing list|set|delete`
  - 控制台：`/admin/pricing`
- 平台用户生命周期管理完成：创建用户、切换状态、重置密码；mutation 走 `Permission::PlatformAdmin` 并写入 `user.*` audit。

### Added — Quota / Billing

- `inflight_requests` 增加 `quota_keys` 与 `estimated_micros`，pre-debit 成功后写入飞行中请求记录。
- 后台 sweeper 每 60s 扫描过期 inflight 记录并退还 Redis budget 预扣，覆盖进程崩溃后的 quota 回滚路径。

### Added — Provider / Plugin

- HTTP Plugin 新增共享 SSE normalizer，支持 CRLF/LF、注释、多行 `data:`、分片帧、`[DONE]` / `EOF` 类结束帧，并把私有 token / finish / usage path 归一成 OpenAI-compatible stream chunk。
- Provider 插件预设落地：`model_mapping.plugin.preset.provider` 支持 `openai`、`openai_compatible`、`anthropic_messages`、`azure_openai`、`gemini`、`deepseek`、`mistral`、`cohere_chat`、`ollama`、`groq`、`together`、`openrouter`、`moonshot`、`zhipu`、`qwen`、`yi`、`bedrock_converse` 等。
- 预设会补齐默认 path / headers / request adapter / response mapper / SSE mapper；OpenAI-compatible 自动注入 `stream_options.include_usage=true`，Azure 支持 deployment path 模板，Anthropic Messages / Bedrock Converse 具备基础 request adapter。
- HTTP Plugin manifest 按不可信配置硬化：header/path/body 模板分域白名单、绝对 `chat_path` 默认禁用、内网/metadata host 拒绝、request/response/SSE event size limit。

### Changed — Frontend / DX / CI

- 前端抽出 `$lib/design/classes.ts` 与页面模板：`PageShell`、`AuthFrame`、`SectionCard`、`StatePanel`、`ModalFrame`、`DataToolbar`、`FilterPanel`、`DataTable`。
- Channel UI 增加 Provider 插件预设选择，仍保留自定义 plugin manifest 输入。
- CI 改为稳定 Rust toolchain，持续跑 `git diff --check`、`cargo fmt`、`cargo clippy --workspace --all-targets -D warnings`、`cargo check --workspace`、`cargo test --workspace`、`npm run check`、`npm test` 与 Web build；Actions runtime 强制 Node 24，Web job 使用 Node 22。

### Tests

- 当前 Rust 测试清单增至 277 entries（272 unit/integration + 5 doctest）；前端 Vitest 增至 55 tests。
- 新增覆盖：plugin preset 后端单测/集成测试、Anthropic/OpenAI-compatible preset 归一链、plugin manifest 安全护栏、admin 用户 E2E、typed ID/FlexUuid、crash-safe quota pre-debit、pricing rules API 与前端 API helper。

### Added — Release / Docs

- 新增 `ROADMAP.md`，明确“先收口、再补全能力、最后打磨”，并把渠道插件化列为核心竞争力。
- 新增 `docs/plugin-manifest.md`，冻结 HTTP Plugin manifest v0 边界，覆盖 OpenAI-compatible、Anthropic Messages、Azure OpenAI 与私有 SSE token frame 示例。
- 新增 `RELEASE.md` 与 `docs/security-runbook.md`，固化发布、回滚、密钥轮换、Redis quota 异常与 HTTP Plugin 风险处置流程。
- `kgctl doctor` 增强为发布前体检：校验 `KOOIX_PUBLIC_URL`、数据库 migration 最新版本，以及 Redis rate-limit/quota Lua 脚本可执行。

### Resolved from 0.1.5 Known Limitations

- typed ID response 已落地。
- Pricing rules API、CLI 与前端管理页已落地。
- `inflight_requests` 已接入 quota pre-debit crash recovery。
- WASM 插件 ABI 仍延后，HTTP Plugin manifest + Provider 预设继续作为当前扩展面。

## [0.1.5] — 2026-05-15

从 v0.1.0 到 0.1.5，大量功能增强和 bug 修复。覆盖 9 provider 多模态、可视化编排、多维度计费、全面 UI 重做。

**Workspace**: 9 crates · 24 migrations · 241 tests (all green) · SvelteKit 控制台全功能

### Added — Provider 插件架构

- 9 provider 适配器：OpenAI / Anthropic / Azure / Gemini / DeepSeek / Mistral / Groq / Moonshot / Bedrock
- Tool calling + Embeddings + Models 列表 API
- Anthropic Messages API ↔ OpenAI 格式双向翻译
- Gemini REST API 适配（role mapping + part 结构转换）
- 每 provider 独立超时、重试、参数覆写

### Added — 路由策略增强

- 5 种路由策略：`priority` / `weighted_random` / `round_robin` / `least_conn` / `least_latency`
- Channel Group fallback 链（最深 5 级，防环）
- Model filter：`supported_models` + `model_filter` 双层匹配
- Channel RPM/TPM 限速（滑动窗口，超限自动跳下一个）
- 滑动窗口成功率追踪 + 自动禁用（低于阈值自动标记 disabled）
- Model alias 路由（alias → target_model 翻译）
- Channel balance 管理（余额不足自动跳过）

### Added — 多维度计费引擎

- `pricing_rules` 表：dimension × unit × conditions JSON 匹配
- 支持维度：`prompt_tokens` / `completion_tokens` / `cached_tokens` / `reasoning_tokens` / `images_generated` / `audio_seconds_in` / `tts_characters` 等
- `conditions` JSONB 匹配：quality / size / cache_ttl / context_above / batch / region
- Priority + channel specificity 排序，`ROW_NUMBER() OVER (PARTITION BY dimension)` CTE
- 自动同步 LiteLLM 定价数据（启动时 + 每 24h 从 GitHub 拉取 `model_prices_and_context_window.json`）

### Added — 可视化编排 Playground

- @xyflow/svelte 节点式流程编辑器（取代原有 tab 式 Playground）
- 8 种节点：TextInput / ImageUpload / AudioUpload / LLMChat / ImageGen / TTS / STT / Preview
- 拓扑排序 DAG 执行引擎
- 左侧节点面板 + 拖放 + 4 个快速启动模板
- localStorage 持久化（可选云端同步预留）
- Handle 百分比定位（自适应端口数量）

### Added — 控制台全面重做

- Channel 管理：创建/编辑/健康检查/导入导出/全局仪表盘
- Channel Key 加密存储 + 轮转
- Channel Group 管理 + 绑定编辑
- API Key CRUD + 撤销
- Quota CRUD（org/project/api_key 多级）
- 月度账单 + CSV 导出 + 配额告警
- 请求日志：20+ 维度高级过滤 + Dashboard 统计（Admin 面板）
- Usage 仪表盘增强：sparkline + 模型排行 + 错误列表
- Org / User / Project 完整 CRUD
- Settings 页面（密码修改等）
- ModalityBadge 组件：自动检测模型类型（Chat / Image / TTS / STT / Embedding）

### Added — UI 设计系统

- Monochrome zinc-only 调色板 + 语义色（green / amber / red）
- Inter + JetBrains Mono 字体
- lucide-svelte 统一 icon + Provider 品牌色 SVG logo（20 个）
- Dark mode 全面适配（class-based + anti-FOUC inline script）
- Sidebar 浅色米白 / 深色暗黑
- 全宽布局（移除 max-w 限制）
- DropdownMenu fixed positioning（解决 overflow 裁剪）
- ProviderSelect combobox 组件

### Added — 运维增强

- `kgctl setup`：交互式首次引导
- Docker Compose 一键部署（Dockerfile + docker-compose.yml）
- GitHub Actions CI（测试 + Docker 构建 + Release 工作流）
- OpenTelemetry tracing + Prometheus metrics endpoint
- RLS 强化（quota 表 + 审计隔离）
- 审计日志：关键操作自动记录

### Added — 安全增强

- Channel key envelope encryption + KMS 解密路由
- RLS 全表激活 + gate_app 角色隔离
- 审计日志跨 Org 隔离

### Fixed

- 上游 Auth 错误正确映射为 502（修复 `AppError::Internal` 吞掉类型信息）
- Quota scope check 约束补全（加入 `api_key` + `membership` scope_kind）
- Channel group strategy check 对齐（`weighted_random` 替代 `weighted`）
- 测试 fixture FK 约束修复（usage_records / outbox_consumer / RLS 测试）
- `apiFetch` 导入修复（channel detail 页使用 `getChannelStats` 导出函数）
- Provider logo 品牌色（替代 `dark:invert` hack）
- Sidebar 浅色模式米白色
- Flow editor handle 百分比定位
- Playground dark mode 完整适配

### Tests

- 241 测试全绿（unit + integration）
- testcontainers 17-alpine（`KOOIX_TEST_PG_TAG` env override）
- wiremock 假装上游 OpenAI / OIDC IdP
- InMemory repo 与 Pg repo 双实现契约测试
- 前端 vitest 50 测试

### Known Limitations at 0.1.5

- API response 返裸 UUID，typed ID 前缀格式待下版本迁移
- Pricing rules API + CLI CRUD 延后到下一迭代
- `inflight_requests` 流式预扣尚未接入 chat handler
- WASM 插件延后

[Unreleased]: https://github.com/telagod/kooix-gate/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/telagod/kooix-gate/compare/v0.1.5...v0.2.0
[0.1.5]: https://github.com/telagod/kooix-gate/compare/v0.1.0...v0.1.5
[0.1.0]: https://github.com/telagod/kooix-gate/releases/tag/v0.1.0
