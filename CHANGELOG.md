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
- Plugin runtime 开始按 `auth.strategy` 注入认证：`bearer` / `api_key_header` / `api_key_query` / `basic` / `custom_headers` / `none`，preset 会自动映射 Azure `api-key`、Anthropic `x-api-key` 与 Bedrock 临时 header。

### Changed — Docs

- 整理文档入口：新增 `docs/README.md` 与 `docs/stages/README.md`，把已完成的重构审计记录归入 `docs/stages/`，保留 active waivers 原路径供 CI / quality gate 使用。
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
