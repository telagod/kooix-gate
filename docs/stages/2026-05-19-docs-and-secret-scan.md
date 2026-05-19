# 文档分层与 Secret Scan 收口

Status: applied
Scope: 文档入口清理、阶段性文档归档、gitleaks 本地安装复验、HTTP Plugin secret slots 收口。
Last verified: 2026-05-19

## 关键文档 vs 阶段性文档

- 关键入口保留在根目录与 `docs/README.md` 索引：`README.md`、`DESIGN.md`、`ROADMAP.md`、`CHANGELOG.md`、`RELEASE.md`、`AGENTS.md`、`CLAUDE.md`、`docs/plugin-manifest.md`、`docs/security-runbook.md`、`docs/observability-runbook.md`。
- 模块文档保留在对应模块：`web/README.md`、`web/src/lib/design/README.md`、`crates/kgctl/README.md`、`bench/README.md`、`examples/README.md`。
- 已完成的一次性审计、迁移、收口和验证快照统一放入 `docs/stages/`，不再散落根目录。
- active waiver 仍放在 `docs/waivers/`，因为脚本 / CI 可能引用，暂不归档到 stages。

## gitleaks

- 本机安装位置：`/home/telagod/.local/bin/gitleaks`
- 版本：`8.30.1`
- CI：`.github/workflows/ci.yml` 的 `Security Smoke` job 已使用 `gitleaks/gitleaks-action@v2`。

本地验收命令：

```bash
gitleaks version
gitleaks detect --source . --redact --verbose
tmp=$(mktemp -d) && git ls-files -co --exclude-standard -z | tar --null -T - -cf - | tar -C "$tmp" -xf - && gitleaks detect --source "$tmp" --no-git --redact --verbose
```

说明：`--no-git --source .` 会扫描 `.env` 与 `target/` 等 gitignored 本地文件；用于泄露排障时有价值，但不代表仓库可提交内容。本轮仓库口径采用 git history + tracked/unignored working tree 两条扫描。

## Plugin secret slots

本轮把 P1.1.2 的 “Secret 来源统一”、`hmac`、`aws_sigv4` 与 `oauth_client_credentials` 从 TODO 收口为代码路径：

- `CustomHttpProvider::new_with_secret_slots` 接收 slot map，`new_with_opts` 继续兼容旧 primary API key。
- `ProviderRouter::resolve_secrets_for_channel` 读取同一 channel 的 active `channel_keys`，按 `label` 归一为 secret slot 并用 `EnvelopeKms` 解密。
- `primary` / `api_key` / 空 label 保持旧主密钥语义；非 plugin provider 仍只使用 primary。
- repo/crypto 缺失或 DB 无 active key 时回退 env：`KOOIX_CH_<CODE>_KEY`、`KOOIX_API_KEY`、`KOOIX_PLUGIN_SECRET_<SLOT>`、`AWS_SECRET_ACCESS_KEY`。
- `auth.strategy = "hmac"` 支持 method/path/query/body_sha256/timestamp/nonce 签名 payload，使用 `secret_slot` 做 HMAC-SHA256，并自动注入 timestamp / nonce / signature header。
- `auth.strategy = "aws_sigv4"` 支持 AWS Signature Version 4 canonical request / string-to-sign / signing key，自动注入 `Authorization` / `x-amz-date` / `x-amz-content-sha256` / 可选 `x-amz-security-token`。
- Bedrock Converse preset 默认切到 `aws_sigv4`，不再注入临时 `X-Amz-Access-Key` / `X-Amz-Secret-Key` header。
- `auth.strategy = "oauth_client_credentials"` 支持向 HTTPS `token_url` 发送 client credentials form，用 `client_id_slot` / `client_secret_slot` 换取 access token，缓存到过期前并注入 `Authorization: Bearer <token>`。
- Admin channel test 对 plugin provider 改为传完整 secret slot map；`channel_keys.alias` 新增 slot 字符集校验，避免 UI 写入运行期无法引用的 slot。

验证命令：

```bash
cargo test -p gate-providers router_db_key_decrypt_roundtrip -- --nocapture
cargo test -p gate-providers router_secret_slots_use_channel_key_labels -- --nocapture
cargo test -p gate-providers plugin_auth_uses_explicit_secret_slot_map -- --nocapture
cargo test -p gate-providers plugin_auth_hmac_signs_method_path_body_timestamp_nonce -- --nocapture
cargo test -p gate-providers parses_hmac_auth_manifest_defaults_and_payload_template -- --nocapture
cargo test -p gate-providers hmac_rejects_unknown_payload_template_variable -- --nocapture
cargo test -p gate-providers plugin_auth_aws_sigv4_signs_bedrock_request -- --nocapture
cargo test -p gate-providers parses_aws_sigv4_auth_manifest_defaults -- --nocapture
cargo test -p gate-providers bedrock_preset_defaults_to_aws_sigv4_without_fake_secret_headers -- --nocapture
cargo test -p gate-providers oauth -- --nocapture
cargo test -p gate-providers plugin_env_secret_slots_include_named_plugin_secrets -- --nocapture
cargo test -p gate-providers plugin -- --nocapture
cargo clippy -p gate-providers --all-targets -- -D warnings
cargo clippy -p gate-server --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Plugin Auth 前端表单

本轮把 P1.1.2 的前端 channel auth strategy 配置从原始 manifest 手填推进为可 lint 表单：

- `web/src/lib/components/channels/PluginAuthEditor.svelte`：创建 / 编辑 channel 共用的 Auth Strategy editor。
- `web/src/lib/plugin-presets.ts`：新增 `PluginAuthForm`、默认 preset auth、manifest → form round-trip、`buildPluginAuthManifest` 本地 lint 与 auth 合并逻辑。
- `web/src/routes/channels/+page.svelte`：Plugin provider 创建 / 编辑抽屉按 strategy 展示最小字段，保存前合并 auth 到 manifest；“本地 lint”按钮复用同一构造链。
- 支持策略：`bearer`、`api_key_header`、`api_key_query`、`basic`、`custom_headers`、`hmac`、`aws_sigv4`、`oauth_client_credentials`、`none`。
- 本地 lint 限制：secret slot 仅允许 `[a-zA-Z0-9_-]`；OAuth `token_url` 必须 HTTPS，本地仅放行 `localhost` / `127.0.0.1`；`expiry_skew_seconds` 限制 0-3600；custom headers 必须是非空 JSON object。

验证命令：

```bash
npm --prefix web run check
npm --prefix web test -- plugin-presets
```

## Plugin Request Mapping DSL

本轮把 P1.1.3 的 request mapping 从基础模板推进到可覆盖私有 deployment 的 DSL：

- `request.path` / `request.query` / `request.headers` / `request.body` 模板新增 `tools`、`tool_choice`、`metadata.*`、`extra.*`，body 也支持整段 `metadata` / `extra`。
- 整段占位继续保留 JSON 原类型；缺失、`null`、空字符串、空数组、空对象会在 query/header/body object 中自动跳过，避免私有上游拒绝未知空字段。
- Header 仍保留分域白名单：`{{messages}}` 等大 payload 不能塞进 header，manifest 加载时直接拒绝。
- Anthropic Messages 与 Bedrock Converse preset 继续通过 `adapt_chat_request` 做 message transform，覆盖 system prompt、multimodal parts、tool calls / tool results 基础映射。
- Plugin channel 的 `model_mapping` 可同时保留 `plugin` manifest 与 `models` / `model_aliases` / `deployments` 映射，路由顺序为 project model alias → channel deployment mapping → plugin request 模板。

验证命令：

```bash
cargo test -p gate-providers plugin -- --nocapture
cargo test -p gate-server --test c1_routing plugin_manifest_channel_model_mapping_rewrites_deployment_path -- --nocapture
cargo test -p gate-server --test c1_routing full_chain_rewrites_model_from_alias_and_channel_mapping -- --nocapture
```

## Plugin Response / Usage Mapping

本轮把 P1.1.4 的非流式 response / usage 映射收口为稳定 evaluator 与可对账响应字段：

- 字段路径从简单 dot path 扩展为 `nested.object`、`array.0.index`、`path.a|path.b|default:<json>` first non-null fallback。
- 非流式 response 新增 `reasoning_content_path`、`tool_calls_path`、`request_id_path`、`metadata_path`；`request_id` 与 `upstream_metadata` 会保留在 `ChatResponse`，便于日志 / replay / vendor 对账。
- Usage 新增 `reasoning_tokens_path`、`image_units_path`、`audio_seconds_path`、`raw_path`；`raw_path` 保存 vendor 原始 usage metadata。
- 字段缺失按 0 / fallback 处理；usage 类型不匹配会返回 decode error，避免静默错计费。
- pricing 管理页维度改为后端 `pricing_rules` 实际消费的维度名：`per_image`、`per_minute_audio`、`reasoning_tokens` 等，避免 UI 写入旧维度后计费引擎无法匹配。
- Billing emit 改为直接读取 `pricing_rules` 并用 `compute_cost(CostContext, rules)`，不再只走 legacy `ModelPricing` 的 input/output/cached 三列；reasoning/image/audio 映射出来后可被同名 pricing dimension 消费。

验证命令：

```bash
cargo test -p gate-providers response_mapping -- --nocapture
cargo test -p gate-providers plugin_maps_response_paths_fallback_tool_calls_metadata_and_usage_units -- --nocapture
cargo test -p gate-server --test billing_e2e non_stream_usage_event_keeps_raw_and_multimodal_cost_dimensions -- --nocapture
cargo check -p gate-server
```

## Plugin SSE Normalizer / Replay Harness

本轮把 P1.1.5 的 SSE normalizer 从共享 decoder 推进到 manifest-driven 产品能力：

- `stream.ignore_events` / `stream.done_events`：按 SSE `event:` 名称跳过 heartbeat / ping 或结束分流。
- `stream.done_path` / `stream.done_values`：支持 vendor done object，例如 `{"type":"message_stop"}`，不再只识别 `[DONE]` / `EOF` raw token。
- `stream.tool_calls_path`：私有 tool call delta array 直接映射到 `ChatDelta.tool_calls`。
- `UsageManifest::should_emit_stream_usage`：usage-only 末帧即使只有 prompt / cached / reasoning / raw usage 也可输出；Anthropic output-only streaming 仍避免 message_start prompt-only 帧提前对外暴露。
- `gate_providers::replay_plugin_sse`：后端 / CLI / UI 共用同一回放核心。
- `POST /v1/admin/plugin-manifest/replay`：平台管理员可上传 manifest + raw SSE，返回 OpenAI-compatible chunks。
- `kgctl plugin replay manifest.json --sse sample.sse`：本地 fixture 回放，不需要启动 gate-server。
- Channel 创建 / 编辑抽屉新增 `SSE replay preview`，可直接粘贴 raw SSE 预览归一 chunks。
- `/v1/chat/completions` 流式 billing guard 改为缺 usage 末帧时生成 estimated usage 并写 outbox，`raw_usage.estimated=true`，避免静默漏扣。

验证命令：

```bash
cargo test -p gate-providers replays_manifest_driven_sse_events_tool_calls_usage_and_done_object -- --nocapture
cargo test -p gate-providers plugin_normalizes_event_split_tool_delta_usage_and_vendor_done -- --nocapture
cargo test -p gate-server --test billing_e2e stream_without_usage_frame_emits_estimated_usage_event -- --nocapture
cargo run -q -p kgctl -- plugin replay /tmp/kgctl-plugin-replay/manifest.json --sse /tmp/kgctl-plugin-replay/sample.sse
npm --prefix web test -- plugin-presets
```

## P1.1.7 Manifest Builder / Debugger

本轮把 Manifest Builder / Debugger 从分散的 textarea + replay 入口推进为可验收的 7 步创建流，并补齐 CLI golden fixture 回放：

- `kgctl plugin test`：用 `CustomHttpProvider` 对真实 / mock 上游发一次 non-stream chat，输出归一后的 `ChatResponse`，默认 API key 可读 `KOOIX_PLUGIN_TEST_API_KEY`。
- `kgctl plugin export`：把 manifest、可选 non-stream response sample、raw SSE 与 replay 后的 `expected_chunks` 导出为 v1 golden fixture。
- `kgctl plugin import --verify`：校验 fixture manifest，并重放 raw SSE 与 `expected_chunks` 比对；生成型 `chatcmpl-*` id 会在比较时归一，避免非确定性破坏 golden。
- Channel 创建抽屉新增 7 步 builder：preset/custom → auth → request mapping → response sample 点选字段 → raw SSE replay → probe/test 参数 → 保存并可自动 `addGroupBinding` 加入 group。
- `web/src/lib/plugin-presets.ts` 新增 `PluginBuilderDraft`、`buildPluginBuilderManifest`、`suggestResponsePaths`，让 response sample 可自动建议 `content_path` / `finish_reason_path` / usage paths，也支持手动点选覆盖。
- 前端 `ProbeResponse` 类型补齐 `probe_model` 与 `max_cost_micros`，与后端 P1.1.6 返回结构对齐。
- 新增 `crates/gate-server/tests/channel_plugin_e2e.rs`，覆盖 replay → create plugin channel → group binding 的控制面闭环。

验证命令：

```bash
cargo test -p kgctl plugin_ -- --nocapture
cargo test -p gate-server --test channel_plugin_e2e -- --nocapture
npm --prefix web test -- plugin-presets
npm --prefix web run check
```

## P1.2 Provider Capability Matrix

本轮把 P1.2 的 capability matrix 从路线项落成 runtime / API / UI 共享契约：

- `gate_providers::ProviderCapabilities` 成为内置 Provider 与 HTTP Plugin manifest v1 共用字段，覆盖 `chat`、`streaming`、`tools`、`embeddings`、`image`、`audio`、`vision`、`json_mode`、`batch`。
- `PluginManifest::apply_preset` 会把 preset 的 truthy capability 默认值并入 manifest；旧 v0 / 简写 preset 仍可自动升级。
- Router 新增 `route_chat`，会按请求实际需求跳过不满足 `streaming` / `tools` / `vision` / `json_mode` 的 channel，并在 route decision trace 记录 `missing_capability:*`。
- Embedding route 改为读取 capability matrix，只选择声明 `embeddings=true` 且当前已有 embedding runtime 的内置 Provider。
- Admin Channel / Group binding API 返回 `capabilities`，控制台在 Channel 列表、创建 / 编辑抽屉和 Group binding 表展示 capability chips。
- Plugin preset 增加 Base URL 建议与本地 / 自托管 OpenAI-compatible 变体：`vllm`、`lm_studio`、`ollama_openai`、`localai`、`xinference`。
- Bedrock Converse 保持 `aws_sigv4` 正式鉴权，capability 先按保守 `chat` / `streaming` 声明。

验证命令：

```bash
cargo test -p gate-providers capability -- --nocapture
cargo test -p gate-providers preset_defaults_fill_capabilities -- --nocapture
cargo test -p gate-providers openai_compatible_variant_presets_parse -- --nocapture
cargo test -p gate-providers route_chat_records_capability_skip_reason -- --nocapture
cargo test -p gate-server --test c1_routing route_chat_skips_channel_missing_requested_capability -- --nocapture
cargo test -p gate-server --test auth_flow admin_can_create_plugin_channel_with_provider_preset_manifest -- --nocapture
cargo test -p gate-server --test channel_plugin_e2e plugin_manifest_builder_flow_creates_fixture_channel_and_group_binding -- --nocapture
npm --prefix web test -- plugin-presets
npm --prefix web run check
```
