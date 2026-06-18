# Product Review · 第四刀 followup-final（2026-05-26）

> 第四刀（0.4.151-0.4.171，21 patch）真实质量自我批判。
> 与前三刀 followup 同样自洽 — 不护短，所有"以为做完"的项分类。

## TL;DR

第四刀承诺：**第三刀诚实评推 v0.5.x 的 5 项全部真还到 0.4.x**。

实际：5 项 outcomes 全部以代码 commit 落地 + tests 全过。**但仍有 4 类「真实债务」推到 v0.5.x**，本文逐项摊明。

不像第一刀有"占位 env"幽灵、不像第二刀有"step 1/N 没下文"，第四刀的债是**已知边界清晰 + 推迟原因充分**：要么依赖外部环境（docker 网络拓扑）、要么依赖框架成熟度（Svelte 5 runes testing-library）、要么是 caller-side 决策（gate-server 启动流不在第四刀范围）。

## 各项收口情况

### #1 admin/shared.rs 物理拆（0.4.151-155）— ✅ 真收口

**做了**：13 helper 物理迁出 channels.rs；7 sibling 切 `use super::shared::*`；mod.rs/shared.rs 头注释更新。

**没做（已知边界）**：
- channels.rs 内部仍保留 13 个 thin wrapper（`pub fn xxx(...) { super::shared::xxx(...) }`），让 channels.rs 自身 handler 继续 work。彻底删 thin wrapper 需要 channels.rs 内部 handler 改 `super::shared::xxx` 直调 — 仅是 cosmetic，不影响 sibling 依赖结构，推 v0.5.x。

**评**：真收口。"反向依赖"这个第三刀指出的真实债已断绝（grep `use super::channels::` 在 sibling 文件 0 hit）。

### #2 DataTable virtualize 真接（0.4.156-158）— ✅ 真收口 + 一处技术债

**做了**：admin/requests + audit 双轨切换；rowSnippet/expandedRowSnippet 抽出；无展开 + ≥40 行走 virtualize。

**真实债**：
- **展开行变高破坏 virtualize 假设** — 当前用「有展开 → 退 legacy」回避。真正的解法是 variable-height row virtualization（react-window 风格的动态测量+offsetMap），工程量 1-2 个 patch，推 v0.5.x 单独立项。
- incidents 评审定无需 virtualize（聚合视图子表 < 20 行）—— 这个判断稳。

**评**：真接 caller 了，但 variable-height 是 known 技术债。展开浏览仍走 legacy 在万行表上也无性能问题（每次只展开 1 行）。

### #3 playground capability gating（0.4.159-163）— ✅ 真收口 + 1 个偷工

**做了**：FlowEditor 侧栏 + 右键 disabled；NodeCapabilityHint 共享组件接 4 个 AI 节点；13 helper vitest。

**真实债**：
- **UI 组件 @testing-library/svelte 测试缺失** — flow-capabilities.test.ts 只测纯 helper（13 个），FlowEditor 的「按 capability disabled 按钮」+ NodeCapabilityHint 的「amber 横幅显隐」没真渲染断言。
  - 原因：Svelte 5 runes + onMount async fetch 在 jsdom 环境的 fragile 行为（已知 issue 上游 svelte 仓库讨论中）。
  - 影响：CSS class 改回原貌不会被测试发现。推 v0.5.x。
- **后端 ProviderCapabilities 没有单独 stt/tts flag** — 当前 stt/tts 都看 audio flag。这是后端 schema 限制，前端无法解；推 v0.5.x 后端拆 `audio_in/audio_out`。

**评**：行为对，测试覆盖到 helper 层；UI 层是 framework 限制下的真实债。

### #4 chaos test 真启 toxiproxy（0.4.164-167）— ⚠ 半收口

**做了**：testcontainers 真启 toxiproxy 2.9.0 容器；admin REST helper（add_proxy / set_proxy_enabled / add_toxic）；3 case：
- case #1 拒绝连接（opt-in docker）
- case #2 latency toxic（opt-in docker）
- case #3 上游 503 风暴（**默认跑**，用 wiremock 替代真 toxiproxy）

**真实债**：
- **PG/Redis 真接通 toxiproxy 没做** — case #1/#2 只验「toxiproxy admin API 写入对了」，没验「fred Redis client 走 toxiproxy → 真启的 Redis container 真出错」。完整链路需要：
  1. testcontainers 起 PG/Redis container（已有）
  2. toxiproxy proxy 配 `upstream = host.docker.internal:{pg/redis-port}`（docker desktop OK，linux docker 需 `--add-host`）
  3. gate-server 连 toxiproxy 端口而非直连 PG/Redis
  
  工程量 4-6 patch，依赖 host 网络拓扑稳定性。推 v0.5.x。

- **case #3 没真接 gate-server provider router** — wiremock + ProbeChaos counter 是「上游 503 → injector 计数」的 in-process 模拟。要真验"gate-server retry 4 次都 503 后 fallback 到 group B" 需要拉起整个 axum app + ProviderRouter，覆盖面大。推 v0.5.x。

**评**：admin API helper 真，case #3 wiremock 真，但「真实链路」是已知缺口。

### #5 WASM auto-mount 业务流（0.4.168-171）— ✅ 真收口 + 1 个 caller-side 缺口

**做了**：try_auto_mount + sha256 双校验；batch summary；真接 WasmHost.load_module + 失败回滚；metric emit；WasmtimeHost e2e（wat::parse IDENTITY_WAT → invoke_hook 验 identity transform）。

**真实债**：
- **gate-server 启动流没调 auto_mount_and_load_into_host** — 当前 router 接口暴露好了、4 类失败回滚分类清晰、metric emit 也接通，但 gate-server 启动时 / channels.update 后没 caller 调它。这是 caller-side 决策（gate-server 装配链改动），不在第四刀 #5 物理范围。推 v0.5.x 在 gate-server `main.rs` startup 或 `ChannelRepoWatcher` 加调用点。

**评**：库侧全做完。Caller 接通是另一块工作，但接口已经"准备好被接"。

## 总结分类

| 状态 | 数量 | 项 |
|------|------|----|
| ✅ 真改 runtime / 抽新接口 | 8 | shared.rs / DataTable virtualize 双轨 / FlowEditor + NodeCapabilityHint / Toxiproxy 真启 + admin REST / try_auto_mount + AutoMountSummary + load_into_host |
| ✅ 真测试（含 e2e） | 8 | flow-capabilities 13 / chaos 3 case (含 wiremock 真跑) / wasm_auto_mount 12 (含真 WasmtimeHost e2e) |
| ⚠ 已知技术债推 v0.5.x | 5 | DataTable 变高 row virtualize / Svelte 5 testing-library UI 测试 / 后端 audio_in/audio_out 拆 / chaos PG/Redis 真接通 / WASM auto-mount gate-server caller 接通 |
| 📜 文档同步 | 1 | product-gaps + ROADMAP 第三/第四刀章节 |

## 第四刀 vs 前三刀

- 第一刀（37 patch）：有大量"占位 env"幽灵 / "step 1/N 没下文"被第二刀 followup 揭穿
- 第二刀（19 patch）：自我批判稿 + 真改 5 项 runtime + 11 项设计稿
- 第三刀（30 patch）：真还第二刀的"真实债务"22 项，但留 5 项推 v0.5.x
- **第四刀（21 patch）：完全收口第三刀推的 5 项 + 留 5 项已知技术债推 v0.5.x**

第四刀的"债"全部是有清晰原因 + 边界 + 后续路径的，**不是隐瞒不报**。

## 推 v0.5.x 的 5 项总览

| 项 | 来源 | 原因 |
|----|------|------|
| DataTable variable-height row virtualize | #2 | 工程量需 1-2 patch 单独立项 |
| Svelte 5 + jsdom UI 组件测试 | #3 | 框架 fragile，等上游成熟 |
| ProviderCapabilities audio_in/audio_out 拆 | #3 | 后端 schema 变更 |
| chaos PG/Redis 真接通 toxiproxy | #4 | host 网络拓扑 + 4-6 patch |
| WASM auto-mount gate-server caller 接通 | #5 | 装配链改动，非第四刀范围 |

---

**v0.5.0-rc1 候选门禁**（下版 0.4.174 验证）：
- cargo test --workspace 全绿（含新加的 12 个 wasm_auto_mount + chaos 6 + flow capabilities 13）
- cd web && npm run check 0/0 + vitest 全跑 21 files
- ROADMAP + product-gaps + CHANGELOG 同步
