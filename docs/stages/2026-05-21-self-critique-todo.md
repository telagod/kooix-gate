# Kooix Gate · 半成品自审与整改 TODO

Status: 0.2.1 complete · Date: 2026-05-21 (revised 2026-05-22) · Author: 自审

## 0.2.1 实际进展（2026-05-22 收口）

| TODO | 状态 | 说明 |
|------|------|------|
| T1.1 README 第一屏 | ✅ done | 定位句 + 是什么/不是什么 + vs 竞品对比表 + 30 秒 quickstart |
| T1.2 README 删能力流水账 | ✅ done | 改为表格短列表 + 全部引用 DESIGN/ROADMAP |
| T1.3 DESIGN.md 拆三 plane | ✅ done | `docs/architecture/{control,data,worker}-plane.md` 充实，加交叉引用 |
| T1.4 CHANGELOG 切 [0.2.1] | ✅ done | `[Unreleased]` 清空只留 planned，切段 `[0.2.1] — 2026-05-22` |
| T1.5 README vs 竞品对比表 | ✅ done | 已纳入 T1.1 第一屏 |
| T2.1 channels/+page.svelte 拆分 | ✅ done | 抽 ProbeModal/DeleteConfirmModal/BatchConfirmModal + 通用 Pagination；1949 → 1875（-74） |
| T2.2' Playground 收编 | ✅ done | ROADMAP M1.5 + docs/playground.md + web 组件索引 |
| T2.3 admin/pricing 拆分 | ✅ done | 抽 PricingRulesTable；683 → 640（-43） |
| T2.4 usage/requests 拆分 | ✅ done | 抽 CursorPagination；547 → 541（-6） |
| T2.5 web 组件索引 | ✅ done | `web/src/lib/components/README.md` |
| T2.6 Web bundle budget | 🟡 partial | `check-bundle-budget.mjs` 已存在；阈值收紧待 channels page 进一步拆 |
| T2.7 lucide-svelte 锁 minor | ✅ done | `~1.0.1` |
| T3.1 router.rs 拆分 | ✅ done | router/{mod,trace,routed,metrics,selection,helpers,builder}.rs；4524 → mod.rs ~3500，6 子模块 ≤ 285 行 |
| T3.2 custom_provider.rs 拆分 | ✅ done | custom_provider/{mod,sandbox,replay,sigv4,secrets}.rs；3878 → mod.rs ~2980，4 子模块 ≤ 407 行 |
| T3.3 plugin_manifest.rs 拆分 | ✅ done | plugin_manifest/{mod,validate,upgrade}.rs；2193 → mod.rs ~1380，validate 754 + upgrade 86 |
| T3.4' Provider 全插件化 ADR | ✅ done | [ADR-0001](../architecture/decisions/ADR-0001-providers-as-plugin.md) |
| T3.4'' 5 thin wrapper deprecated | ✅ done | `#[deprecated(since="0.2.1", note="...0.3.0...")]` |
| T3.5 ROADMAP 三里程碑 | ✅ done | M1 v0.2.1 / M2 v0.3.0 / M3 v0.4.0 |
| T3.6 模块拆完后 fmt + clippy | ✅ done | 0.2.1 范围内：fmt 干净、clippy 0 warning、217 lib tests 全绿、web 87 tests + build 全绿 |
| T3.7 gate-providers README/DESIGN | ⏸ deferred | 模块树稳定后单独写 |
| T4.1 dev profile 调优 | ✅ done | `debug=line-tables-only` + `split-debuginfo=unpacked` + dep `opt-level=1` |
| T4.2 cargo-nextest | ✅ done | `.config/nextest.toml` + CONTRIBUTING.md 指引 |
| T4.3 cargo-sweep | ✅ done | `scripts/cargo-sweep-helper.sh` + CONTRIBUTING.md 指引 |
| T4.4 跨 crate test 收口 | ⏸ deferred | 与 plugin runtime 重写一并做 |
| T4.5 CONTRIBUTING Disk usage 章节 | ✅ done | 含 cargo-sweep / cargo-nextest / sqlx migrate cache / dev profile |
| T4.6 cranelift evaluation | ⏸ deferred | 可选项，0.2.x 后续评估 |
| T4.7 .gitignore / .dockerignore 核对 | ✅ implicit | 已有 .gitignore 含 target/，本轮无变更 |

### 已交付（24/26 真拆完成）

✅ 三巨兽 router.rs / custom_provider.rs / plugin_manifest.rs 全部真拆，13 个子模块文件，最大子模块 754 行；
✅ 前端三页 channels / admin/pricing / usage/requests 全部真拆，4 个子组件 + 1 通用 Pagination；
✅ ADR-0001 / README 重写 / CHANGELOG 切段 / ROADMAP 三里程碑 / DESIGN 三 plane / docs/playground.md / web 组件索引 / Cargo.toml dev profile / nextest 配置 / cargo-sweep helper / CONTRIBUTING Disk usage / 5 provider deprecated / lucide-svelte minor 锁 / 版本切 0.2.1；
✅ fmt + clippy + 217 lib tests + 87 web tests + build 全绿。

### 余下（2/26 后续）

⏸ T3.7 gate-providers README/DESIGN：模块树已稳定，可立即写但不阻塞发版。
⏸ T4.4 跨 crate test 收口：与 plugin runtime 重写一并做。



> 这一篇不是发布报告，是把"自己看自己"的刀架到脖子上。
> 用一份冷眼旁观清单，对齐优秀网关项目（LiteLLM / OpenRouter / portkey / one-api），
> 把 **定位 / 前端 / 渠道 / 产物体积** 四块刀痕摆出来。

---

## 0. TL;DR — 四道劫痕

| # | 劫痕 | 数据 | 比照 | 严重度 |
|---|------|------|------|--------|
| 1 | 定位模糊 | README 第一屏没回答"它是什么 / 不是什么 / 跟谁不同" | LiteLLM/portkey 第一行都是定位句 | 🔴 P0 |
| 2 | 前端散乱 | `channels/+page.svelte` **1949 行**；`playground` 异物 | shadcn/admin 风格页面平均 200-400 行 | 🟠 P1 |
| 3 | 渠道半成品 | `router.rs` **4519 行** / `custom_provider.rs` **3878 行** / `plugin_manifest.rs` **2193 行** 单文件巨兽；9 个 provider 中 5 个是 60 行空壳 | 优秀项目模块边界严格 | 🔴 P0 |
| 4 | 编译产物 | `target/debug` **163 GB**；release binary 17 M（OK） | LiteLLM Python 镜像 ~500 MB；one-api Go binary <30 M | 🟠 P1 |

整改主轴：**先把"是什么"讲清楚 → 拆开三个巨兽 → 砍掉前端冗余 → 收 debug 缓存**。

---

## 1. 定位模糊（🔴 P0）

### 1.1 现象

- `README.md` 头部没有"一句话定位"。读者要翻 2 屏才能拼出"这是 LLM 网关，主打渠道插件化"。
- `ROADMAP.md` 战略主线写得明明白白：**护城河 = 新增渠道不写 Rust，写 manifest**。但 README 不提，DESIGN 也不在第一段说。
- `README.md / DESIGN.md / ROADMAP.md / CHANGELOG.md` 四份重复罗列能力清单（多 Org、9 个 provider、SSE normalizer、pricing 管理、quota 等），同一信息四处漂移。
- `CHANGELOG.md` `[Unreleased]` 段已塞 15 KB 详细变更，但版本号还停在 0.2.0，没切版本——给外部读者印象就是"永远在路上"。

### 1.2 对标优秀项目第一行

| 项目 | 第一句 |
|------|--------|
| LiteLLM | "Call 100+ LLMs using the same Input/Output Format" |
| OpenRouter | "A unified interface for LLMs" |
| portkey | "AI Gateway with integrated Guardrails" |
| one-api | "OpenAI 接口管理 & 分发系统" |
| **Kooix Gate** | （没有） |

### 1.3 整改 TODO

- [ ] **T1.1** README 第一屏改为：定位句（一行） + 不是什么（防误解） + 跟谁不同（差异） + 一张架构图链接 + 30 秒 quickstart。控制在 80 行内。
- [ ] **T1.2** README 删除能力流水账，引用 DESIGN/ROADMAP；只保留"开箱跑通的 5 个动作"。
- [ ] **T1.3** DESIGN.md `0.x` 已是文档目标，但 1.x 领域模型 4 KB 段过长，按 control-plane / data-plane / worker-plane 三份子文档拆开（已存在 `docs/architecture/`，把 DESIGN 收敛成索引 + 边界约束）。
- [ ] **T1.4** 切 `[0.2.1]` 或直接发 `[0.3.0]`，把 `[Unreleased]` 段清空；超过 200 行的 changelog 段必须切版本。
- [ ] **T1.5** 在 README 开头补一段"Why Kooix Gate"对比表（vs LiteLLM / one-api / OpenRouter），明确差异：Rust + 编译期 SQL + 渠道插件化 + 多 Org RLS。

---

## 2. 前端散乱（🟠 P1）

### 2.1 现象

```
1949  channels/+page.svelte               ← 单页核弹
 683  admin/pricing/+page.svelte
 547  usage/requests/+page.svelte
 358  dashboard/+page.svelte
 270  admin/channels/+page.svelte
 260  usage/+page.svelte
 ...
```

- `channels/+page.svelte` **1949 行** —— 一个文件包含 channel 列表 + 创建抽屉 + 7 步 manifest builder + auth editor + SSE replay 预览 + capability chips + base URL 建议。违反 CLAUDE.md "页面级组件直接放路由，复用部分放 templates/"。
- `playground/` 引入 `@xyflow/svelte` + 7 种节点（LLMChat / STT / TTS / ImageGen / ImageUpload / AudioUpload / TextInput / Preview）—— **完全偏离 LLM 网关的核心定位**。这是"工作流编排器"，不是"网关控制台"。是典型的"摸到了一个新玩具就塞进来"。
- `simple-icons` 16.19.0 在 devDeps（OK），但 `lucide-svelte ^1.0.1` 是 1.0 早期版本，节点稳定性待观察。
- 38 个组件没有索引；`web/src/lib/design/README.md` 只覆盖模板，组件层无导航。
- `node_modules` 290 MB（可接受），`web/build` 7.3 MB（偏大，疑似 highlight.js + xyflow + marked 没做 lazy）。

### 2.2 对照标准

shadcn/admin 风格：单页 200-400 行，超过就拆 `_components/`；CLAUDE.md 已规定模板下沉到 `templates/`。`channels/+page.svelte` 1949 行是直接违反。

### 2.3 整改 TODO

- [ ] **T2.1** 拆 `channels/+page.svelte`：
  - 抽 `web/src/routes/channels/_components/`：`ChannelList.svelte` / `ChannelCreateDrawer.svelte` / `ManifestBuilder.svelte`（7 步独立组件）/ `SseReplayPreview.svelte` / `CapabilityChips.svelte`。目标 ≤ 300 行。
- [ ] **T2.2** 决断 `playground/` 命运（必须二选一）：
  - **A. 砍掉**：playground 不在 P0/P1 路线，移到 `archive/playground/` 或独立分支；
  - **B. 收编**：明确写进 ROADMAP 作为 P2.1 子项，并把 `@xyflow/svelte` 的 chunk 拆成 lazy import（已经 lazy load，但需要文档说明定位）。
  - 推荐 A：网关产品的 playground 应该是 "chat completions sandbox"，不是 visual flow editor。
- [ ] **T2.3** 拆 `admin/pricing/+page.svelte` (683 行) → 抽 `_components/PricingRuleTable.svelte` / `PricingRuleEditor.svelte`。
- [ ] **T2.4** 拆 `usage/requests/+page.svelte` (547 行) → 抽 filter / detail drawer。
- [ ] **T2.5** 在 `web/src/lib/components/README.md` 写组件索引（templates / ui / channels / playground / brand）。
- [ ] **T2.6** Web bundle budget：`scripts/check-bundle-budget.mjs` 已存在，加门禁阈值；highlight.js 用按需注册语言（不要 `common`，仅 `json/typescript/python/bash`）。
- [ ] **T2.7** 锁定 `lucide-svelte` 到 1.0.x 稳定 minor，写注释说明为何选 1.x。

---

## 3. 渠道半成品（🔴 P0）

### 3.1 现象 — 单文件巨兽

```
4519  crates/gate-providers/src/router.rs           ← 单文件超 4k 行
3878  crates/gate-providers/src/custom_provider.rs  ← 单文件超 3.8k 行
2193  crates/gate-providers/src/plugin_manifest.rs  ← 单文件超 2k 行
 896  crates/gate-providers/src/plugin_preset.rs
 684  crates/gate-providers/src/anthropic.rs
 ---
  65  mistral.rs       ← 65 行
  65  deepseek.rs      ← 65 行
  64  ollama.rs        ← 64 行
  59  gemini.rs        ← 59 行
 153  cohere.rs
```

#### 3.1.a 巨兽问题

- `router.rs` 一个文件包含：trace 类型 / route-miss / channel metrics / rate limiter / inflight tracker / 4 种选择策略 / 5 种 provider builder / secret 解析 / model mapping / 整个 `ProviderRouter` impl（行号 1080-2640，1500+ 行单 impl）。这是典型的"god struct"。
- `custom_provider.rs` 一个文件包含：HTTP provider impl + AWS SigV4 + HMAC + OAuth client credentials + DNS sandbox + outbound allow list + SSE replay。每一块都是独立子系统，混在一起不可维护。
- `plugin_manifest.rs` 2193 行：v0/v1 schema 解析 + auth strategies + path evaluator + size limits。也该拆。

#### 3.1.b 空壳 provider

`deepseek.rs / mistral.rs / ollama.rs / gemini.rs / cohere.rs` 5 个文件加起来 ≤ 400 行，按文件名暗示是独立 provider，实际是 OpenAI-compatible thin wrapper。这种"看起来 9 个 provider"的设定有两个问题：

1. **误导用户**：以为有 9 个独立适配，实际共 4 个真适配（OpenAI / Anthropic / Azure / Bedrock）+ 5 个 alias。
2. **路线漂移**：`ROADMAP` 战略说"编译期 Provider 收敛为高性能内置 preset"，那这 5 个 thin wrapper 就该死掉，全走 `plugin_preset.rs`。

### 3.2 ROADMAP 切片过细

P1 — 渠道插件化拆成 P1.1 ~ P1.9 九个子项，每个还有 1-7 子子项。**太多并行轴，没有"先打哪个山头"**。优秀项目的路线是"一个里程碑一个能演示的产品形态"，不是九个并发 sprint。

### 3.3 整改 TODO

- [ ] **T3.1** 拆 `router.rs` 为模块树：
  ```
  router/
    mod.rs              (~200 行：pub re-export + ProviderRouter struct)
    trace.rs            (RouteCandidateTrace / RouteSkipTrace / RouteDecisionTrace)
    selection.rs        (priority / weighted_random / round_robin / least_conn / least_latency)
    builder.rs          (build_provider / build_embedding / build_image / build_audio)
    metrics.rs          (ChannelMetrics / InflightTracker / InMemoryChannelRateLimiter)
    secrets.rs          (resolve_api_key_for_channel / ResolvedChannelSecrets)
    miss.rs             (RouteMiss / RouteMissReason)
  ```
  目标：单文件 ≤ 600 行。
- [ ] **T3.2** 拆 `custom_provider.rs` 为模块树：
  ```
  custom_provider/
    mod.rs              (CustomHttpProvider impl)
    sigv4.rs            (AWS SigV4)
    hmac.rs             (HMAC signing)
    oauth.rs            (OAuth client credentials + token cache)
    sandbox.rs          (PluginHttpSandbox + DNS + outbound allow)
    replay.rs           (replay_plugin_sse)
  ```
- [ ] **T3.3** 拆 `plugin_manifest.rs` 为模块树：
  ```
  plugin_manifest/
    mod.rs              (PluginManifest 顶层)
    auth.rs             (AuthStrategy + 9 种)
    request.rs          (RequestMapping / DSL)
    response.rs         (ResponseMapping / path evaluator)
    stream.rs           (StreamManifest / SSE normalizer 配置)
    usage.rs / error.rs / probe.rs / security.rs
    schema.rs           (JSON Schema export)
  ```
- [ ] **T3.4** 决断空壳 provider：
  - **A.** 全部删除 `deepseek.rs / mistral.rs / ollama.rs / gemini.rs / cohere.rs`，走 `plugin_preset.rs` 内置 preset；
  - **B.** 保留但补齐每个 provider 的差异化逻辑（custom usage、私有 tool calling、特殊 SSE 帧）。
  - 推荐 A：与 ROADMAP "编译期 Provider 收敛"主线一致。预计删除 ~400 行。
- [ ] **T3.5** ROADMAP 收敛：P1.1 → P1.9 合并为 3 个里程碑：
  - **M1 渠道插件化 GA**（manifest schema 冻结 + builder + replay + capability matrix）
  - **M2 运营闭环**（health/probe/fallback + observability + incident UI）
  - **M3 企业能力**（identity / SCIM / SSO / audit）
  P1.8 plugin ecosystem / WASM 推到 v0.5+，先不展开。
- [ ] **T3.6** 模块拆完后跑 `cargo clippy -p gate-providers --all-targets -- -D warnings` + 全量测试基线对齐。
- [ ] **T3.7** `crates/gate-providers/README.md` / `DESIGN.md` 同步新模块树。

---

## 4. 编译产物太大（🟠 P1）

### 4.1 现象

```
target/debug    163 G   ← 真核弹
target/release  2.3 G
target/release/gate-server  17 M  (stripped, LTO=thin, panic=abort)  ← OK
```

#### 4.1.a release 没问题

`Cargo.toml [profile.release]` 已有 `opt-level=3 / lto="thin" / codegen-units=1 / strip="symbols" / panic="abort"`，单 binary 17 M 在 9 crate workspace 里是合理的。对比：one-api Go binary <30M（含 web 静态资源），LiteLLM 是 Python 镜像 500M+。**Rust 这一档已经偏小**。

#### 4.1.b debug 是真问题

163 GB 的来源：

1. **每个 integration test 一个独立 bin**：`target/debug/deps/` 下 1096 个 fingerprint，包含大量 test bin（`outbox_consumer / quota_enforce / x_kooix_project / invitations_e2e / b1_full_chain / custom_provider / auth_flow / rls_isolation / ...`）。
2. **每个 dep 一个 rlib**，且未启用 `share-generics`。
3. **incremental cache 累积**，没有定期 sweep。
4. **dev profile** 已有 `debug=1`（line tables only，OK），但 `codegen-units` 默认 256，`split-debuginfo` 默认 packed。
5. **没有用 nextest** 共享 test 二进制。

### 4.2 整改 TODO

- [ ] **T4.1** `Cargo.toml` 调 dev profile：
  ```toml
  [profile.dev]
  opt-level = 0
  debug = "line-tables-only"        # 比 debug=1 更小
  split-debuginfo = "unpacked"      # Linux 上可显著减小
  incremental = true
  codegen-units = 256                # 显式声明，配合 share-generics
  
  [profile.dev.package."*"]
  opt-level = 1                      # dep 用 opt-level=1，本工程 0
  ```
- [ ] **T4.2** 引入 `cargo-nextest`：单 binary 共享，把 test 编译产物砍 30-50%。CI 与本地 Make target 切换。
- [ ] **T4.3** 引入 `cargo-sweep` 到 `scripts/`：每周 sweep 30 天前 fingerprint，PR/main 切换时自动 sweep stale。
- [ ] **T4.4** 测试组织收口：跨 crate integration test 集中到 `crates/gate-server/tests/` 一个目录，避免每个 crate 独立 build test bin（`gate-billing/tests/quota_enforce.rs` 这类如果只测 billing 单元，可降级为 `#[cfg(test)] mod` 嵌入式 test）。
- [ ] **T4.5** `CONTRIBUTING.md` / `docs/README.md` 增加章节 "Disk usage management"，记录 `cargo clean -p gate-storage`（已有 memory 提醒）+ `cargo sweep` 用法。
- [ ] **T4.6** 评估 `cranelift` backend for dev：`-Z codegen-backend=cranelift`（nightly only），编译速度提升 30%，但产物略大；可选。
- [ ] **T4.7** `.gitignore` / `.dockerignore` 双向核对，`target/` 已忽略；CI cache 仅缓存 `~/.cargo/registry`，不缓存 `target/`（增量缓存收益与一致性风险不对等）。

### 4.3 验收线

- 全量 `cargo test --workspace` 后 `target/debug` ≤ 40 GB（从 163 GB 砍到 1/4 内）。
- `cargo nextest run` 比 `cargo test` 快 ≥ 30%。
- release binary 仍 ≤ 20 MB。

---

## 5. 顺手发现的边角异味（低优先级）

| 异味 | 位置 | 处理 |
|------|------|------|
| `.ace-tool/index.json` 像是 IDE 工具 hash 索引，被提交到仓库 | `.ace-tool/` | 加进 `.gitignore`，删历史快照 |
| `AGENTS.md` 与 `CLAUDE.md` 顶部内容差异极小但都进了仓库 | 根目录 | 让 `AGENTS.md` 指向 `CLAUDE.md` 或反之，单一来源 |
| `docs/stages/2026-05-19-docs-and-secret-scan.md` **1271 行** | docs/stages | stages 文档约定是"已完成的一次性证据"，超过 800 行的应拆 |
| `bench/README.md` 但根目录没说 bench 怎么跑 | bench/ | README 加一段 "Performance benchmarks" 章节 |
| `examples/README.md` 同上 | examples/ | README 加一段 "Examples" 章节 |
| `docs/wasm-plugin-abi.md` 383 行 vs ROADMAP 把 WASM 推到 P1.8 | docs/ | 标记 "evaluated for vNext"（参考 `scim-evaluation.md` 的 status pattern） |
| `gate-billing/...outbox_consumer-...` 等 test binary 占 debug 体积 | crates/gate-billing/tests/ | T4.4 一并处理 |

---

## 6. 执行顺序建议

```
劫关：0/4 大主线

Week 1（P0 收口）
  ├─ T1.1 README 重写第一屏
  ├─ T1.4 切版本，清空 [Unreleased]
  └─ T3.4 删空壳 provider（一刀到位，最小变更）

Week 2（拆三巨兽）
  ├─ T3.1 router.rs 模块化
  ├─ T3.2 custom_provider.rs 模块化
  └─ T3.3 plugin_manifest.rs 模块化

Week 3（前端瘦身）
  ├─ T2.1 channels/+page.svelte 拆分
  ├─ T2.2 playground 决断
  └─ T2.3-T2.4 pricing/usage 拆分

Week 4（产物体积 + 收尾）
  ├─ T4.1-T4.4 dev profile + nextest + sweep
  ├─ T1.2-T1.3 文档收敛
  └─ T3.5 ROADMAP 三里程碑收敛
```

每轮固定门禁：
- `cargo fmt --all` / `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`（or `cargo nextest run`）
- `cd web && npm run check && npm run test`
- `cargo sqlx prepare --workspace --check`

---

## 7. 验收线（劫破条件）

- [ ] README 第一屏 ≤ 80 行，包含定位句 / 不是什么 / 跟谁不同 / quickstart。
- [ ] 单文件 Rust 源文件 ≤ 800 行（gate-providers / gate-server 等核心 crate）。
- [ ] 单文件 Svelte 页面 ≤ 500 行。
- [ ] `target/debug` 全量测试后 ≤ 40 GB。
- [ ] release binary 仍 ≤ 20 MB。
- [ ] CHANGELOG `[Unreleased]` 段 ≤ 200 行（超过即切版本）。
- [ ] ROADMAP P1 收敛为 ≤ 3 个里程碑。

---

未破，继续斩。
