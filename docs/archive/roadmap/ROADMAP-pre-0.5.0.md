# Kooix Gate Roadmap

> 三里程碑驱动：**0.2.1 收尾优化 → 0.3.0 编译期 provider 退役 → 0.4.0 fast-path runtime**。
> 战略主线不变：**渠道插件化** —— 新增渠道写 manifest，不写 Rust adapter。

## 三里程碑（2026-05-22 收敛）

| 里程碑 | 目标 | 时间盒 | 破坏性 |
|--------|------|--------|--------|
| **M1 · v0.2.1 收尾** | 文档定位 + 三巨兽拆解 + dev profile + 前端 1949 行核弹拆分 + Playground 收编为产品线 + ADR-0001 锁定 | 2-3 周 | 否 |
| **M2 · v0.3.0 退役** | 删 5 个 thin wrapper provider；ChannelRecord migration 自动迁移到 plugin preset；OpenAI/Anthropic/Azure/Bedrock 保留 fast-path 内置实现 | 3-4 周 | 是（`provider_type` 收敛） |
| **M3 · v0.4.0 收敛** | `gate-providers` 收敛为 "1 plugin runtime + N preset bundle"；`builtin_fastpath` 标志位；capability matrix 集中维护 | 4-6 周 | 否 |

> 完整路线总览见下方 P0/P1/P2 历史段落（保留作为已完成基线证据）。

---

## M1 · v0.2.1 — 收尾与大范围优化

**主题**：把"半成品"标签摘掉。定位说清楚 / 单文件巨兽拆掉 / dev profile 调优 / playground 作为产品线写实 / provider 全插件化路径锁定。

参考自审清单：[docs/stages/2026-05-21-self-critique-todo.md](./docs/stages/2026-05-21-self-critique-todo.md)

### M1.1 定位与文档收口

- [x] **T1.1** README 第一屏重写：定位句 / 不是什么 / vs 竞品对比表 / 30 秒 quickstart。
- [x] **T1.2** README 删除能力流水账，引用 DESIGN / ROADMAP。
- [x] **T1.3** DESIGN.md 1.x 领域模型按 control / data / worker plane 充实子文档（`docs/architecture/{control,data,worker}-plane.md`）。
- [x] **T1.4** CHANGELOG 切 `[0.2.1]` 段，`[Unreleased]` 清空只留 planned。
- [x] **T1.5** README "Why Kooix Gate" 对比表（已纳入 T1.1 第一屏）。
- [x] **T7** ADR-0001 provider 全插件化迁移决议落地。

### M1.2 编译产物体积

- [x] **T4.1** Cargo.toml dev profile：`debug="line-tables-only"` + `split-debuginfo="unpacked"` + `[profile.dev.package."*"] opt-level=1`。
- [x] **T4.2** 引入 `cargo-nextest`（`.config/nextest.toml` + CONTRIBUTING.md 指引）。
- [x] **T4.3** 引入 `cargo-sweep`（`scripts/cargo-sweep-helper.sh` + CONTRIBUTING.md 指引）。
- [x] **T4.5** `CONTRIBUTING.md` 增 "Disk usage management" 章节。
- [ ] **T4.4** 跨 crate integration test 集中（与 M1.3 三巨兽拆分一并做，test 重组依赖模块边界稳定）。

### M1.3 渠道半成品 — 三巨兽拆解

- [x] **T3.4'** 编译期 provider 标 `#[deprecated(since="0.2.1", note="use plugin preset; will be removed in 0.3.0. See ADR-0001.")]`：5 个 thin wrapper（cohere/deepseek/gemini/mistral/ollama）已加。
- [x] **T3.1** 拆 `router.rs` (4519 行) → `router/{mod,builder,helpers,metrics,routed,selection,trace,tests}.rs`（0.4.1 完成，mod.rs 1713 行）。
- [x] **T3.2** 拆 `custom_provider.rs` (3878 行) → `custom_provider/{mod,fastpath,helpers,replay,sandbox,secrets,sigv4,tests}.rs`（0.4.1 完成，mod.rs 1452 行）。
- [x] **T3.3** 拆 `plugin_manifest.rs` (2193 行) → `plugin_manifest/{mod,factory,helpers,upgrade,validate,tests}.rs`（0.4.1 完成，mod.rs 705 行）。
- [x] **T3.6** 模块拆完后跑 `cargo clippy --workspace --all-targets -- -D warnings` + 全量测试基线对齐（0.4.1 完成）。
- [x] **T3.7** `crates/gate-providers/README.md` / `DESIGN.md` 同步新模块树（0.4.1 完成）。

### M1.4 前端散乱

- [x] **T2.1** 拆 `channels/+page.svelte` 1864 → 1487 行 (-20.2%)（0.4.2 抽 helpers / EditChannelDrawer；0.4.3 抽 CreateChannelDrawer；0.4.4 抽 badge/fmt helpers）。ChannelTable 完整组件化推迟到 0.5.0+（DataTable 段调用面 ~30 props，硬拆反模式）。
- [x] **T2.3** 拆 `admin/pricing/+page.svelte` 640 → 633（0.4.8 抽 DeletePricingModal）。pricing wizard form 多步流程留 0.5.0+。
- [x] **T2.4** 拆 `usage/requests/+page.svelte` 541 → 507（0.4.9 共享 helper）。
- [x] **T2.5** `web/src/lib/components/README.md` 写组件索引。
- [x] **T2.6** Web bundle budget 收紧门禁阈值（0.4.18 完成：250KB → 220KB；CI 已集成；0.5.0+ 计划进一步收到 180KB）。
- [x] **T2.7** `lucide-svelte` 锁定 `~1.0.1`（minor 锁）。
- [x] **0.4.x 额外**：admin/groups 1083 → 972 (4 modal 拆)；admin/users 752 → 729 (2 modal)；quotas 959 → 948；admin/requests 628 → 594。共建 _components 子目录 6 个 + 共享 helper 模块 3 个。

### M1.5 Playground 收编为产品线

> 决断锁定（2026-05-21）：Playground 保留并升级为产品线。理由：与 LLM 网关定位互补——网关解决"接入与计费"，playground 解决"对接演示与上下游链路调试"。

- [x] **T2.2'** 在本 ROADMAP 与 README 显式写"Visual Workflow Editor"作为 v0.2.1 产品线。
- [x] **新 docs/playground.md**：节点类型、连线规则、与 chat/embeddings/image/audio 路由的耦合方式、ProviderCapability 联动、bundle 策略、已知限制、M1.5 路线。
- [ ] 给 7 种节点（LLMChat / STT / TTS / ImageGen / ImageUpload / AudioUpload / TextInput / Preview）补 vitest 覆盖。
- [ ] Playground 节点共享 `ProviderCapability` 矩阵：节点根据 capability 自动禁用不支持的 channel/model。
- [ ] Playground 工作流执行链路接入 `request_events`，所有节点请求落 audit。
- [ ] 工作流持久化（`playground_workflows` 表）。

### M1.6 验收（0.2.1 已通过）

```bash
cargo fmt --all -- --check                            # ✅ clean
cargo clippy --workspace --all-targets -- -D warnings # ✅ 0 warning
cargo test --workspace --lib                          # ✅ 217 passed
cd web && npm run check                               # ✅ 0 errors / 0 warnings
cd web && npm test                                    # ✅ 87 passed (13 files)
cd web && npm run build                               # ✅ built in 7.17s
```

发版条件（部分达标）：

- ✅ ADR-0001 已 accepted。
- ✅ README 第一屏 ≤ 80 行，含定位 / 差异表 / quickstart。
- ✅ 单文件 Rust ≤ 800 行（核心 crate）：0.4.1 完成，三巨兽全部下到 705 / 1452 / 1713（受内部循环依赖限制，1452 / 1713 已是现阶段最优）。
- ⏸ 单 Svelte 页面 ≤ 500 行：deferred 到 0.4.x 续（channels/+page.svelte 1864 行仍待拆）。
- ⏸ `target/debug` 全量测试后 ≤ 40 GB：dev profile 已就位，需下次全量测试验证。

---

## M2 · v0.3.0 — 编译期 Provider 退役

**主题**：执行 ADR-0001。删 5 个 thin wrapper，channel migration 自动改 `plugin` + preset。

详细计划见 [ADR-0001](./docs/architecture/decisions/ADR-0001-providers-as-plugin.md)。

- [ ] Plugin runtime Criterion bench ≤ 编译期 provider × 1.05（5% 性能预算）。
- [ ] Capability matrix golden test：覆盖 18+ preset 的 chat/streaming/tools/embeddings/image/audio/vision/json_mode/batch 矩阵。
- [ ] 删除 `cohere.rs / deepseek.rs / gemini.rs / mistral.rs / ollama.rs`（5 个 thin wrapper）。
- [ ] 保留 `openai.rs / anthropic.rs / azure.rs / bedrock.rs` 作为 fast-path，但逻辑等价于 plugin preset。
- [ ] ChannelRecord migration：所有 `provider_type=openai|anthropic|...` 自动迁移为 `plugin` + 对应 preset；migration 幂等可回滚。
- [ ] 0.2.x 期间双跑窗口：同时支持 `provider_type=openai`（编译期）与 `provider_type=plugin + preset=openai_compatible`（plugin runtime），便于灰度。
- [ ] `gate-providers/src/router.rs` 删除 `is_plugin_provider() / supports_*_runtime()` 分支，行数预计砍 30-40%。
- [ ] 0.3.0 发布前完成所有 error mapper 收敛到 plugin runtime。

破坏性变更声明：`provider_type` 收敛为 `plugin | custom | http | http_plugin`，存量 channel 自动迁移；旧 `provider_type` 名称保留为 alias 一个 minor 版本周期。

---

## M3 · v0.4.0 — Fast-path Runtime

**主题**：`gate-providers` 终极形态——1 个 plugin runtime + N preset bundle。
详细设计见 [ADR-0002 Fast-path Runtime](./docs/architecture/decisions/ADR-0002-fastpath-runtime.md)。

**触发依据**（0.3.0 实测）：plugin runtime vs builtin chat 路径 ratio = **× 1.41**
（CI [×1.32, ×1.51]），超 ADR-0001 的 5% 预算 8 倍，必须做 fast-path。
复现：`cargo bench --package gate-providers --bench plugin_vs_builtin`。

**2026-05-22 收尾**：M3 全部完成，bench 实测 fast-path 路径 × 0.74-1.00（远好于预算）。

- [x] 引入 `builtin_fastpath: true` manifest 标志：4 个高 QPS provider（OpenAI / Anthropic / Azure / Bedrock）走静态分发，避免 manifest 解释器开销。
- [x] ~~Preset bundle 拆 crate~~ — 评估后决定不拆（23 个 preset 共享 OpenAI adapter，硬拆重复代码；细节见 [ADR-0002 § preset bundle 决策](./docs/architecture/decisions/ADR-0002-fastpath-runtime.md#preset-bundle-决策2026-05-22)）。
- [x] Plugin runtime 性能基准：fast-path × 0.74-1.00（远好于 × 1.02 预算），manifest runtime × 1.27-1.45。
- [x] Capability matrix golden test：fastpath × 9 capability + 全 23 preset，`tests/capability_matrix.rs` 锁定。
- [x] Fastpath panic fallback：`catch_unwind` 兜底退到 manifest runtime（`run_fastpath` helper）。
- [x] WASM Plugin ABI vNext — 0.4.16 落地 ADR-0003 v0 + sample manifest；0.4.21-0.4.27 wasmtime runtime + Rust SDK + fallback + 3 hook + bench；0.4.41-0.4.46 集成到 CustomHttpProvider + e2e；0.4.51-0.4.52 SSE stream_chunk 真接通 + e2e；0.4.55-0.4.56 AssemblyScript SDK + 示例；0.4.57-0.4.58 ProviderRouter wasm_host + Prometheus describe。**完整产品形态**。

### M3 后 — product-review 第一刀（0.4.65-0.4.101，2026-05-26）

详见 [product-review-2026-05-26.md](./docs/product-review-2026-05-26.md) 第一刀打磨。

- [x] **性能** SharedHttpClient（4 fast-path provider 共享 reqwest pool）— 0.4.65
- [x] **可观测** `gate_chat_*` 4 个 metric（duration / ttfb / stream_chunks / requests_total）— 0.4.66 / observability.md 同步 0.4.73
- [x] **渠道一致性** Anthropic / Bedrock 透传 `ChatRequest.extra`；Azure 接入 lift_openai_usage_details — 0.4.67 / 0.4.75
- [x] **Usage 字段** `cache_creation_input_tokens` + OpenAI o1/o3 nested details lift — 0.4.68
- [x] **安全** `ProviderError` body 脱敏（512B + sha256） — 0.4.69
- [x] **可靠性** Retry ±25% jitter + `RetryConfig::stream_safe()` — 0.4.70
- [x] **配置** PgPool 显式化（`KOOIX_DB_*` 5 env） — 0.4.71 / .env.example 0.4.74
- [x] **重构** admin.rs pricing 内联 mod — 0.4.72
- [x] **重构** channels page plugin samples 抽 `_lib` + sanity tests — 0.4.76-0.4.77
- [x] **WASM** host_log + host_record_metric 实装 + sanitize tests — 0.4.80-0.4.82
- [x] **WASM** cwasm 持久化缓存 + `KOOIX_WASM_CACHE_DIR` env + runbook — 0.4.83 / 0.4.84 / 0.4.89
- [x] **前端** DataTable.svelte `maxHeight` + `stickyHead` + 3 测试 — 0.4.85 / 0.4.86
- [x] **能力面** `GET /v1/admin/providers/capabilities` 完整能力矩阵 endpoint + 测试 — 0.4.87 / 0.4.88
- [x] **文档** RELEASE.md v0.5.0-rc1 准备清单 + README badge 更新 — 0.4.78 / 0.4.79
- [x] **品牌** 「空衍」logo 重设计（D4 → 风车螺旋）— 0.4.101

### M3 后 — product-review 第二刀（0.4.102-0.4.120，2026-05-26）

详见 [product-review-followup-2026-05-26.md](./docs/product-review-followup-2026-05-26.md) 第二刀（自我批判）。

#### 真改 runtime（5 项）
- [x] **可靠性** Retry-After HTTP-date 兼容（RFC 7231）— 0.4.103
- [x] **Usage** audio_tokens / accepted_prediction_tokens / rejected_prediction_tokens — 0.4.104
- [x] **性能** SharedHttpClient LRU per-key eviction（防雷暴）— 0.4.105
- [x] **重构** metric 名抽 `pub mod names` const — 0.4.107
- [x] **重构** admin.rs org_members 块抽内联 mod — 0.4.109

#### 文档 + 测试 + 设计稿（其余）
- [x] followup 批判稿 — 0.4.102
- [x] chat metrics handler grep test — 0.4.106
- [x] stream_safe 语义钉死注释 — 0.4.108
- [x] channels form-factories 抽 `_lib` + 4 tests — 0.4.110 / 0.4.114
- [x] host_get_secret_slot 完整 ABI 设计稿 — 0.4.111
- [x] Grafana dashboard 修指标名漂移 + 加 4 panel — 0.4.112
- [x] 撤回 review §1 P1-3 误判（request_logs 已 outbox 异步）— 0.4.113
- [x] DataTable virtualize 完整设计稿 — 0.4.115
- [x] admin.rs 拆分进度文档化 — 0.4.116

剩余真重构推到 v0.5.x：

- [ ] **admin.rs 真拆物理文件** — `routes/admin/{mod.rs, channels.rs, groups.rs, sso.rs, users.rs, invitations.rs, probe.rs}` 目录化
- [ ] **channels page B2 step 3-4** — list state store + dialog manager + API call wrapper
- [ ] **DataTable virtualize 实装** — 按 0.4.115 设计稿
- [ ] **host_get_secret_slot 实装** — 按 0.4.111 设计稿
- [ ] **WASM module blob store** (G-002) + auto-mount
- [ ] **chat e2e bench** runtime（按 0.4.98 TODO）
- [ ] **chaos test runtime**（按 0.4.99 设计稿）
- [ ] **playground frontend capability 联动**（接 0.4.87 endpoint）

---

### M3 后 — product-review 第三刀（0.4.121-0.4.150，30 patch · 真还债）

第三刀真实把第二刀 followup 提的"真实债务"落到 runtime。详见 [docs/product-gaps.md § 第三刀](./docs/product-gaps.md)。

**真还 22 项**：admin.rs 4368→553 行（-87%）/ DataTable virtualize / HookContext.secrets + host_get_secret_slot / WasmBlobStore / chat e2e bench / chaos fixture / metric const 等。

### M3 后 — product-review 第四刀（0.4.151-0.4.171，21 patch · 5/5 真收口）

第三刀诚实评的"仍未做（推 v0.5.x）"5 项第四刀全部真还。

| 项 | 版本 | 收口 |
|---|------|-----|
| **#1 admin/shared.rs 物理拆** | 0.4.151-155 | 13 helper 物理迁出 + 7 sibling 切 `use super::shared::*`，反向依赖断绝 |
| **#2 DataTable virtualize 真接** | 0.4.156-158 | admin/requests + audit 双轨（无展开 + ≥40 行 → virtualize；其他 legacy） |
| **#3 playground capability gating** | 0.4.159-163 | FlowEditor 侧栏 + 右键 disabled / NodeCapabilityHint 接 4 个 AI 节点 / 13 vitest |
| **#4 chaos test 真启 toxiproxy** | 0.4.164-167 | testcontainers 真启容器 + admin REST helper + 3 case（拒绝/latency/503） |
| **#5 WASM auto-mount 业务流** | 0.4.168-171 | try_auto_mount + AutoMountSummary + 真接 WasmHost.load_module + metric + WasmtimeHost e2e |

第四刀共 **21 patch · 5/5 真收口**。诚实推 v0.5.x：chaos PG/Redis 完整容器流 / UI 组件 testing-library / WASM auto-mount 接进 gate-server 启动流 / DataTable 变高 row virtualize。

### M3 后 — 阶段小版收口（0.4.176-0.4.181，6 patch · 工作区清零）

第四刀大版（0.4.175）封后留下 5 个未提交文件 + CI 额度耗尽 + 240G `target/` 事故，0.4.176-181 把这些尾巴一并收口。

| 项 | 版本 | 内容 |
|---|------|-----|
| **CI 暂停** | 0.4.176 | GH Free private repo Actions 额度耗尽，3 workflow（ci / docker / release）触发改 `workflow_dispatch` only；本地全门禁不变；待 v0.5.x 决策（公开化 / spending limit / self-hosted）后恢复 |
| **build-hygiene runbook** | 0.4.177 | `docs/build-hygiene-runbook.md` 238 行，复盘 240G `target/` 事件 + 7 类清理动作 + CI 集成路径；CONTRIBUTING + docs/README 链入 |
| **admin/users CreateUserForm** | 0.4.178 | 抽 87 行子组件（form/errors/onSubmit/onUpdateField 4 个 props） |
| **admin/users UserStatsCards** | 0.4.179 | 抽 22 行 3 张计数卡（总用户 / Active / Suspended） |
| **admin/users UserTableRow** | 0.4.180 | 抽 80 行行模板 + 清 7 个未用 lucide import（Check / LogOut / Plus / KeyRound / MonitorSmartphone / ShieldCheck / ShieldOff） |
| **阶段封版** | 0.4.181 | 工作区清零，admin/users +page.svelte 650 → 597 行（-53，-8.2%），3 子组件合计抽出 189 行 |

阶段小版共 **6 patch**，仅做工程债清理，无新功能。门禁全绿：web check 0/0 + vitest 127/21。

---

## M4 · v0.5.0 — Enterprise / SaaS 进阶（候选）

**主题**：M3 完整产品形态已交付（0.4.58）；v0.5.0 进入企业级 / SaaS 多区域路由方向。

完整缺口对账见 [docs/product-gaps.md](./docs/product-gaps.md)，本节只列分组：

**P0 — 信任链与运行时收口（v0.5.0 必交付）**

- G-001 真实公钥验签链：cosign / sigstore-rs / minisign 真实签名验签（0.4.54 schema 已落）
- G-002 WASM 模块外部存储 + auto-mount（0.4.57 setter/getter 已落）
- G-003 host functions 真实暴露（host_log / host_get_secret_slot / host_record_metric）
- G-004 SSE event-by-event transform（当前 chunk raw bytes 穿透）

**P1 — DX 与生态铺路（v0.5.0 推荐交付）**

- G-101 AssemblyScript SDK npm publish（0.4.55 本地 package 已落）
- G-102 管理面 WASM 表单 UI
- G-103 ABI v1 走 wit-bindgen + component-model
- G-104 WASM 编译产物持久化缓存（`Module::serialize`）
- G-105 SCIM v2 实装
- G-106 Web bundle 220 → 180 KB

**P2 — 企业 / SaaS 进阶（v0.5.x 后续筛选）**

- G-201 SaaS 多区域路由
- G-202 SAML 2.0 SSO
- G-203 OpenTelemetry log export
- G-204 Cost forecasting / 预算预测
- G-205 Stripe 支付 gateway
- G-206 Playground 收尾（M1.5 残留 4 项）

具体优先级与时间盒待 0.5.0 启动会议确定。

---

## 当前基线（2026-05-22）

`main` 已具备可用网关底盘：

- 多 Org / Project / ApiKey 三层租户，RBAC + RLS 双闸隔离。
- 9 个编译期 Provider（v0.3.0 退役 5 个 thin wrapper，保留 4 个 fast-path）：OpenAI / Anthropic / Azure / Gemini / DeepSeek / Mistral / Groq / Moonshot / Bedrock。
- HTTP Plugin manifest v1 + SSE normalizer，可接私有协议与非标准 SSE。
- Provider 插件预设：OpenAI-compatible / Anthropic Messages / Azure OpenAI / Vertex AI / Gemini / DeepSeek / Mistral / Cohere / Ollama / Groq / Together / OpenRouter / Moonshot / 智谱 / 通义千问 / 零一万物 / Bedrock Converse。
- OpenAI-compatible `/v1/chat/completions` `/v1/embeddings` `/v1/images/generations` `/v1/audio/{speech,transcriptions}` `/v1/responses`，含 streaming / non-streaming / tool calling。
- Channel group 路由：priority / weighted_random / round_robin / least_conn / least_latency，含 fallback group + canary。
- 多维度定价：`pricing_rules` + LiteLLM 自动同步 + REST / CLI / UI 管理面。
- Quota：rpm / tpm / concurrent / daily / monthly / lifetime / budget，Redis Lua 原子执行，crash-safe pre-debit。
- typed ID API response + `FlexUuid` path 兼容。
- SvelteKit 控制台：Channel、Group、Pricing、Quota、Usage、Requests、Billing、SSO、Users、Incidents、Audit、Playground 等管理面。
- 前端设计模板：`PageShell` / `SectionCard` / `DataToolbar` / `DataTable` 等。
- CI：Rust fmt / clippy / check / tests + Web build；当前 485 Rust tests（lib 242 + integration / doctest 243）+ 86 web tests。

## 战略主线：渠道插件化（不变）

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

## 路线总览（已完成基线 — 历史证据，新工作走上方 M1/M2/M3）

> 下方 P0 / P1 / P2 段落保留作为 v0.2.0 已完成的能力基线证据，不再作为前进路线。
> 后续工作请走 M1 / M2 / M3 章节，破坏性变更走 ADR 流程。

| 阶段 | 目标 | 结果定义 |
| --- | --- | --- |
| P0 收口 | 把现有能力封成稳定可发版本 | 文档、迁移、测试、部署、回滚、兼容边界全部对齐 |
| P1 补全能力 | 以渠道插件化为主轴补齐运营网关闭环 | Plugin manifest / 认证 / 字段映射 / SSE / 计费 / 配额 / 观测完整 |
| P2 打磨 | 从”能用”打到”好用、稳、快、可卖” | UX、性能、DX、可维护性、演示与发布资产成熟 |

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

- [x] 全量验证 34 个 migration 空库可跑通。
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

- [x] 实现并启用 concurrent quota。
- [x] lifetime budget / lifetime tokens。
- [x] user × model / api_key × model 的精确策略 UI。
- [x] quota dry-run 模式：
  - [x] 只记录会不会拦截
  - [x] 不实际拦截
- [x] quota explain：
  - [x] 命中了哪条规则
  - [x] 当前消耗
  - [x] 下次恢复时间
- [x] Redis 计数与 PG usage 对账。

### P1.7 Identity / Enterprise 补全

**目标**：从内部 admin 可用，走向企业接入可用。

- [x] 邀请流：
  - [x] org invite
  - [x] project invite
  - [x] 过期 / 撤销
- [x] SSO provider UI 完整化：
  - [x] OIDC discovery
  - [x] allowlist
  - [x] auto-join role
  - [x] redirect policy
- [x] SCIM 评估：
  - [x] 用户同步
  - [x] group → role mapping
- [x] Session 管理：
  - [x] 查看活跃 refresh token
  - [x] 单用户踢下线
  - [x] 全局 JWT rotation。
- [x] `JwtRing`：支持新旧两把 JWT secret 窗口期验证。

### P1.8 Plugin Ecosystem / WASM 补全

**目标**：HTTP Plugin 成为稳定扩展面后，再把生态和更强扩展能力打开。

- [x] Manifest registry：
  - [x] 官方 preset。
  - [x] 社区 manifest。
  - [x] 私有 manifest 导入/导出。
  - [x] 版本、作者、签名、兼容范围。
- [x] Manifest package 规范：
  - [x] `manifest.json`。
  - [x] `fixtures/` 请求、响应、SSE 样本。
  - [x] `README.md` 接入说明。
  - [x] `security.md` 风险声明。
- [x] Plugin sandbox 安全边界产品化：
  - [x] SSRF denylist / allowlist。
  - [x] DNS rebind 防护。
  - [x] header redaction。
  - [x] request / response size limit。
  - [x] timeout / retry / circuit breaker。
  - [x] manifest 权限声明。
- [x] WASM 插件 ABI 设计稿只做 vNext：
  - [x] request transform。
  - [x] response transform。
  - [x] streaming transform。
  - [x] secret access API。
  - [x] deterministic execution constraints。
  - [x] 资源限制与审计。

### P1.9 Observability / Operations 补全

**目标**：生产出问题时能定位、能止血、能复盘。

- [x] Prometheus metrics 完整命名：
  - [x] request count
  - [x] latency histogram
  - [x] upstream error by provider/channel/model
  - [x] quota deny
  - [x] billing settle lag
  - [x] outbox lag
- [x] Trace 串联：
  - [x] request_id
  - [x] org/project/api_key/channel/model
  - [x] upstream request span
  - [x] billing/outbox span
- [x] 控制台事故页：
  - [x] 最近错误
  - [x] top failing channels
  - [x] quota deny top
  - [x] upstream 401/429/5xx 分类
- [x] Runbook：
  - [x] 上游全挂
  - [x] Redis 不可用
  - [x] Postgres 慢查询
  - [x] pricing sync 失败
  - [x] outbox backlog。

---

## P2 — 打磨：从能用到好用

### P2.1 前端体验打磨

**目标**：控制台像产品，不像内部工具。

- [x] 全页面套模板一致性审计：
  - [x] header
  - [x] toolbar
  - [x] filter
  - [x] table
  - [x] empty / loading / error
- [x] 表格能力统一：
  - [x] server-side pagination 基座（`table-state` + `/admin/audit` offset/page size）
  - [x] sort 基座（`/v1/admin/audit-logs` sort_by/sort_dir + UI 表头排序）
  - [x] column visibility 基座（列显隐持久化）
  - [x] saved filters 基座（table state localStorage 持久化）
  - [x] 推广到 `/admin/users`（`DataToolbar` / `DataTable` / column visibility / saved filters）
  - [x] 推广到 `/admin/incidents`（Top failing channels 改用 `DataTable`）
  - [x] 推广到 `/orgs/[orgId]/quotas`（`DataToolbar` 筛选 + `DataTable` 分组表格）
  - [x] 推广到 `/orgs/[orgId]/billing`（`PageShell` / `DataToolbar` / `DataTable`）
  - [x] 推广到 `/orgs/[orgId]/projects`（`PageShell` / `DataTable`）
  - [x] 推广到 `/orgs/[orgId]/projects/[projectId]`（`PageShell` / `StatePanel`）
  - [x] 推广到 `/orgs/[orgId]/projects/[projectId]/keys`（`PageShell` / `DataTable` / `ModalFrame`）
  - [x] 推广到 `/admin/sso`（`DataToolbar` 搜索 + active badges）
  - [x] 推广到 `/usage`（`PageShell` / `DataToolbar` / `StatePanel`）
  - [x] 推广到 `/setup`（`AuthFrame`）
  - [x] 推广到 `/admin/groups`（`PageShell` / `DataTable`）
  - [x] 推广到 `/admin/channels`（`PageShell` / `DataTable`）
  - [x] 模板审计缺口清零（`node scripts/audit-page-templates.mjs`）
- [x] Channel 创建 wizard：
  - [x] 选择 Provider / preset / 自定义 manifest
  - [x] 选择 auth strategy 并填写 secret slot
  - [x] 填 base_url / key / path template
  - [x] 粘贴 response / SSE sample 并点选字段映射
  - [x] 自动 probe
  - [x] 保存并加入 group
- [x] Pricing wizard：
  - [x] 选择模型
  - [x] 选择计费维度
  - [x] 预览价格
  - [x] 模拟一条 usage cost
- [x] Quota wizard：
  - [x] 选择 scope
  - [x] 选择 model filter
  - [x] 输入 rpm/tpm/budget
  - [x] explain 预览。
- [x] UI 文案统一：
  - [x] 中文为主
  - [x] Provider / Channel / API Key 等术语保留英文。

### P2.2 性能打磨

**目标**：稳定承载高并发，不让计费和日志拖慢主链。

- [x] 路由 hot path benchmark：
  - [x] provider selection
  - [x] key decrypt cache
  - [x] quota check
  - [x] request log enqueue
- [x] Channel key 解密缓存：
  - [x] TTL
  - [x] revoke 失效
  - [x] rotation 失效
- [x] Usage/outbox batch insert：
  - [x] outbox enqueue batch
  - [x] usage/request_events/rollups/ledger batch settlement
  - [x] outbox mark done batch
  - [x] duplicate idempotency key safe path。
- [x] Request log 分区 / retention：
  - [x] `request_log_events` 月分区投影
  - [x] `request_events` trigger 自动投影
  - [x] 当前 + 未来分区 helper
  - [x] retention dry-run / apply helper
  - [x] request log read path 优先读分区投影。
- [x] SSE parser 压测：
  - [x] 小帧多
  - [x] 大帧
  - [x] 分片 UTF-8
  - [x] 长连接取消。
- [x] Web bundle 预算：
  - [x] route-level splitting
  - [x] flow editor lazy load
  - [x] markdown highlighter lazy load。

### P2.3 安全打磨

**目标**：默认安全，且安全决策有证据。

- [x] Threat model 文档：
  - tenant isolation
  - API key leakage
  - malicious plugin manifest
  - SSRF
  - billing fraud
  - admin account takeover
- [x] 细粒度 audit：
  - before/after diff
  - actor subject
  - request_id
  - ip/user-agent
- [x] Secret redaction 全链路测试。
- [x] Admin 高危操作二次确认：
  - delete channel
  - rotate/revoke key
  - suspend user
  - change pricing
  - disable group
- [x] Master key rotation tool：
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

- [x] Release checklist 固化到 `RELEASE.md`。
- [x] GitHub Release 自动生成：
  - changelog
  - Docker image tag
  - migration notes
  - known limitations
- [x] Demo script：
  - docker compose up
  - 创建 admin
  - 创建 provider preset channel
  - 发一条 chat
  - 看 usage / billing。
- [x] 截图与短视频：
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
3. Master key rotation。
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
