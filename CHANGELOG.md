# Changelog

All notable changes to **Kooix Gate** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [0.4.130] — 2026-05-26

**Type**: docs · **主题**：admin/mod.rs 头文档更新反映 B1 完成态。

### Changed

- `admin/mod.rs` 头部 doc 从"拆分进度"改为"拆分完成"：
  - 9 个子文件表格（mod.rs 553 / channels.rs 853 / groups 846 / sso 600 / users 529 / probe 488 / invitations 278 / pricing 169 / org_members 100）
  - 共享 helper 通过 `pub(super) use super::channels::{...}` 暴露给 sibling 的设计说明
  - 进一步抽 admin/shared.rs 推到 v0.5.x 的理由（避免 sibling 之间 channels.rs 形成事实共享反向依赖）

### Why

shared.rs 物理拆分推到 v0.5.x：当前 channels.rs 既装业务 handler 又是 helper 库，事实上让其它 mod 反向依赖 channels.rs。最终干净方案是单独的 admin/shared.rs，但本步先文档化现状。

诚实评：未做 admin/shared.rs 实际拆分。

---

## [0.4.129] — 2026-05-26

**Type**: refactor · **主题**：admin/channels.rs 物理拆出（B1 真还债 step 8/8 · 最后一块）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/channels.rs`（853 行）：
  - 17 handler pub(super)（plugin_manifest_{schema,replay} + list/create/update/delete channels + batch_*_channels + drain/get_drain_status/disable_when_idle + channel_keys 4 个 + get_channel_stats）
  - 13 helper pub(super)（record_to_summary / *_audit_snapshot / require_confirmation / audit_meta / channel_capabilities / channel_inflight / is_plugin_provider / key_fingerprint / validate_channel_key_alias 等）
- `admin/mod.rs` 1398 → 553（**真减 845 行**）
- 主 router 17 处改 channels::*
- 7 个 sibling mod（invitations / probe / sso / groups / users / org_members / pricing）加 `use super::channels::{...}` 引共享 helper，标 `#[allow(unused_imports)]` 避免 warning

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

### 累计 admin/mod.rs 收口

| Step | Patch | mod.rs 行数 | Δ | 累计 Δ |
|------|-------|------------|---|--------|
| 起点 | 0.4.120 | 4368 | — | — |
| pricing | 0.4.122 | 4204 | -164 | -164 |
| org_members | 0.4.123 | 4104 | -100 | -264 |
| invitations | 0.4.124 | 3834 | -270 | -534 |
| probe | 0.4.125 | 3352 | -482 | -1016 |
| sso | 0.4.126 | 2759 | -593 | -1609 |
| groups | 0.4.127 | 1920 | -839 | -2448 |
| users | 0.4.128 | 1398 | -522 | -2970 |
| **channels** | **0.4.129** | **553** | **-845** | **-3815** |

**8 个 patch 把 admin god file 4368 → 553，真减 87%**。剩 553 行是 router definition + 共享 helper（v0.4.130 再做 shared.rs 收口）。

---

## [0.4.128] — 2026-05-26

**Type**: refactor · **主题**：admin/users.rs 物理拆出（B1 真还债 step 7/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/users.rs`（529 行）：
  - 11 handler pub(super)（list_audit_logs / list_all_orgs / create_org / update_org / list_users / create_user / update_user_status / reset_user_password / list_user_sessions / revoke_user_session / revoke_user_sessions）
  - 多个 helper（default_limit pub(super), default_audit_sort_*, parse_audit_sort_by, parse_sort_dir, normalize_user_email, user_to_view, org_to_view 等）
- `admin/mod.rs` 1920 → 1398（**真减 522 行**）
- 主 router 8 处改 users::*
- sso.rs 的 default_limit serde default 改 `super::users::default_limit`

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

累计 admin/mod.rs: 4368 → 1398 = **真减 2970 行（68%）**

---

## [0.4.127] — 2026-05-26

**Type**: refactor · **主题**：admin/groups.rs 物理拆出（B1 真还债 step 6/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/groups.rs`（846 行）：
  - 10 handler pub(super)（list/create/update/delete groups + list/add/remove/update bindings + get_group_detail + set_project_default_group）
  - 6 helper（validate_group_strategy / parse_channel_group_id / validate_canary_percent_bps / binding_to_view / deserialize_optional_json_patch / parse_canary_percent_bps_patch）
  - 多 type（GroupView / BindingView / BuildFallbackChain 等）
- `admin/mod.rs` 2759 → 1920（**真减 839 行**）
- 主 router 6 处改 groups::*

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

累计 admin/mod.rs: 4368 → 1920 = **真减 2448 行（56%）**

---

## [0.4.126] — 2026-05-26

**Type**: refactor · **主题**：admin/sso.rs 物理拆出（B1 真还债 step 5/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/sso.rs`（600 行）：
  - 5 handler pub(super)（list / create / update / delete / discover identity providers）
  - 1 internal fn seal_idp_secret
  - 1 view fn identity_provider_to_view
  - 14 normalize / parse helper（non_empty / slug / https_url / scopes / claim / org_role / domain_allowlist / redirect_policy 等）
  - 2 type（IdentityProvidersQuery / RedirectPolicyView 等保 pub 跨文件 serde 可见）
- `admin/mod.rs` 3352 → 2759（**真减 593 行**）
- 主 router 4 处改 sso::*

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

累计 admin/mod.rs: 4368 → 2759 = **真减 1609 行（37%）**

---

## [0.4.125] — 2026-05-26

**Type**: refactor · **主题**：admin/probe.rs 物理拆出（B1 真还债 step 4/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/probe.rs`（488 行）：
  - 3 个 handler（probe_channel_models / test_channel / get_channel_balance）pub(super)
  - 4 个内部类型（ProbeResponse / TestChannelQuery / TestResponse / BalanceResponse）pub(super)
  - 5 个内部 fn（extract_probe_model_ids / update_channel_balance / resolve_probe_key / resolve_probe_secrets / normalize_probe_secret_slot pub(crate)）
- `admin/mod.rs` 3834 → 3352（**真减 482 行**）
- 主 router 3 处改用 `probe::*`
- mod.rs 内 test 引用 normalize_probe_secret_slot 改 `probe::normalize_probe_secret_slot`

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

---

## [0.4.124] — 2026-05-26

**Type**: refactor · **主题**：admin/invitations.rs 物理拆出（B1 真还债 step 3/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/invitations.rs`（278 行）：
  - 9 个 handler（org / project 各 list+create+revoke，加 create_invitation / revoke_invitation 内部 helper）
  - 5 个内部 helper（default_invitation_ttl_hours / invitation_to_view / generate_invitation_token / invitation_accept_url / org+project_role_to_invite_str / ensure_project_in_org）
  - 6 个 router-facing handler 标 `pub(super)`
  - `default_invitation_ttl_hours` 标 `pub(super)` 让父 mod struct serde default 引用
- `admin/mod.rs` 4104 → 3834（**真减 270 行**）
- 主 router 4 处 `.route()` 改用 `invitations::*` 限定路径

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

---

## [0.4.123] — 2026-05-26

**Type**: refactor · **主题**：admin/org_members.rs 物理拆出（B1 真还债 step 2/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/org_members.rs`（100 行，3 handler）
- `admin/mod.rs` 4204 → 4104（**真减 100 行**）

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed
```

---

## [0.4.122] — 2026-05-26

**Type**: refactor · **主题**：admin/pricing.rs 物理拆出（B1 真还债 step 1/8）。

### Changed

- 新增 `crates/gate-server/src/routes/admin/pricing.rs`（169 行）
  - 含 3 个 handler（list_pricing_rules / upsert_pricing_rule / delete_pricing_rule）
  - 含 1 个内部类型 UpsertPricingRuleRequest + 1 个 helper rule_to_row
  - `use super::*` 拿到 admin/mod.rs 顶层的 PricingRulesQuery / PricingRuleRow / audit_meta / require_confirmation / pricing_rule_audit_snapshot
- `crates/gate-server/src/routes/admin/mod.rs` 4368 → 4204 行（**真减 164 行**）
  - 删 inline `mod pricing { ... }` 块
  - 加 `mod pricing;` 单行声明

### Why

第二刀 0.4.72 / 0.4.109 是"inline mod 假拆"——admin.rs 行数 +13 还自称"逻辑边界清晰"。第三刀真还债：物理拆出，使 mod.rs 真正减重。pricing 块最独立、依赖 helper 少，作 step 1。

### Verification

```bash
cargo check -p gate-server         # 0 errors
cargo test -p gate-server --lib    # 50 passed (无回归)
wc -l crates/gate-server/src/routes/admin/mod.rs    # 4204 (从 4368 减 164)
```

---

## [0.4.121] — 2026-05-26 — 第三刀启动 · 还债

**Type**: refactor · **主题**：admin.rs → admin/mod.rs 物理目录化第 1 步。

### Changed

- `git mv crates/gate-server/src/routes/admin.rs → crates/gate-server/src/routes/admin/mod.rs`
- 文件内容 0 改动，仅 path 变化（git rename detection 保留 blame 历史）

### Why

第二刀 0.4.116 只做了"拆分进度文档化"——admin.rs 仍 4254 行 god file，inline mod 只抽出 pricing + org_members 两小块。第三刀真还债：先建 admin/ 目录壳，给后续 9 个 patch（0.4.122-0.4.130）做家。每个版本物理拆一块出来。

### Verification

```bash
cargo check -p gate-server    # 0 errors
cargo test -p gate-server --lib    # 50 passed (无回归)
```

---

## [0.4.120] — 2026-05-26 — 阶段大版 · 双刀打磨收口

> 19 个 patch（0.4.102 → 0.4.120）的第二刀（自我批判）阶段收口。
> 累计：第一刀 37 + 第二刀 19 = **56 patch（0.4.65-0.4.120）**。

### 第二刀战报（19 patch · 0.4.102-0.4.120）

| 类型 | 项数 | 代表 patch |
|------|-----|-----------|
| **真改 runtime** | 5 | 0.4.103 Retry-After HTTP-date / 0.4.104 Usage audio+prediction tokens / 0.4.105 SharedClient LRU per-key / 0.4.107 metric const / 0.4.109 admin org_members inline |
| **修真实 bug** | 1 | 0.4.112 Grafana 用错指标名（`gate_chat_latency_ms` 不存在） |
| **撤回误判** | 1 | 0.4.113 request_logs 已 outbox 异步，撤回 review §1 P1-3 |
| **真画设计图** | 3 | 0.4.111 host_get_secret_slot / 0.4.115 DataTable virtualize / 0.4.116 admin.rs 拆分进度 |
| **测试 / 重构** | 4 | 0.4.106 chat handler 埋点 grep test / 0.4.108 stream_safe 注释 / 0.4.110 form-factories / 0.4.114 form-factories tests |
| **docs / 流程** | 5 | 0.4.102 followup 批判稿 / 0.4.117 SECURITY.md 完整化 / 0.4.118 product-gaps 第二刀 / 0.4.119 README 双刀 / 0.4.120 本次 |

### 自审揭出的 6 类问题（followup §1-§6）

第一刀 37 patch 看似"全收口"，自审揭：

1. **假步骤命名**（admin.rs step 1/4 + channels page step 1/4 + WASM 2/3 + DataTable 1/3 都只做了第 1 步）
2. **占位算实装**（KOOIX_REQUEST_LOG_BUFFER_SIZE / chat e2e bench / chaos-testing.md / playground capability backend-only）
3. **漏网**（Retry-After HTTP-date / audio+prediction tokens / SharedClient 雷暴 / chat metrics 未 e2e 验证 / metric 名 typo）
4. **内联 mod 是假拆分**（admin.rs +13 行还自称"逻辑边界清晰"）
5. **文档与代码不同步**（Grafana 用错指标名 / SECURITY.md 简陋 / RELEASE 检视表混 runtime+docs）
6. **stream_safe 是幽灵 API**（codebase 零调用）

### 第二刀对应修复

P0 真改 runtime 5 项已合（0.4.103 / 0.4.104 / 0.4.105 / 0.4.107 / 0.4.109）；
P1/P2 设计稿与文档已立（0.4.111 / 0.4.115 / 0.4.116 / 0.4.117 / 0.4.118 / 0.4.119）；
余下真重构推 v0.5.x。

### 累计验证（0.4.64 → 0.4.120）

| crate | 0.4.64 | 0.4.120 | Δ |
|-------|--------|---------|---|
| gate-providers tests | 122 | 143 | +21 |
| gate-server tests | 41 | 50 | +9 |
| gate-wasm tests | 13 | 18 | +5 |
| gate-storage tests | 25 | 30 | +5 |
| web tests | 86 | 100+ | +14 |
| **合计 Rust** | **201** | **241+** | **+40** |
| 文档新增 | — | 5 docs | followup / wasm-secret-slot / data-table-virtualize / chaos-testing / product-review |

### 真实债务推 v0.5.x（明示在 ROADMAP）

- admin.rs 物理拆分（5 大块 god file 拆 routes/admin/{...}）
- channels page B2 step 3-4（list state store + dialog manager）
- DataTable virtualize 实装（按 0.4.115 设计稿）
- host_get_secret_slot 实装（按 0.4.111 设计稿）
- WASM module blob store (G-002) + auto-mount
- chat e2e bench 真实装 / chaos test runtime
- playground frontend capability 联动（backend ready 自 0.4.87）

### 阶段亮点

- **诚实优先**：第一刀粉饰被自审揭穿后立即修，followup 批判稿与 ROADMAP 同步对外可见
- **真改 vs 文档化标签**：每个 patch CHANGELOG 顶部标 `**Type:** runtime/test/refactor/design/docs`，让 reader 一眼分辨
- **设计稿钉死方案**：3 个超 patch 范围的特性（host_get_secret_slot / DataTable virtualize / admin 拆分目录化）写完整设计，v0.5.x 实装时无需再讨论
- **追溯 0.4.85 + 0.4.112 真 bug**：Grafana 一直拉的指标名不存在，第二刀复审才发现——证明 followup 自审的真实价值
- **零回归**：56 个 patch 全部 cargo check + 涉及 crate 测试通过；前端 0/0 维持

### 下一步：v0.5.0-rc1

按 [RELEASE.md § rc1 准备清单](./RELEASE.md#v050-rc1-准备清单基于-product-review-2026-05-26) 跑候选门禁。第二刀剩余真重构项进入 v0.5.x 主线。

---

## [0.4.119] — 2026-05-26

**Type**: docs · **主题**：README 当前版本段重写为双刀打磨真实进度。

### Changed

- `README.md § 当前版本` 重写：
  - 标题从 "v0.4.100 — 第一刀打磨完成" 改为 "v0.4.119 — 双刀打磨"
  - 第一刀（37 patch）+ 第二刀（17 patch）= 总 55 patch（0.4.65-0.4.119）
  - **第二刀分类列出诚实评**：真改 runtime 5 项 / 真画图 3 项 / 修真实 bug 1 项 / 撤回误判 1 项 / 测试+重构+文档 7 项
  - **真实债务推 v0.5.x** 7 项明示（admin 物理拆分 / channels page B2 step 3-4 / DataTable virtualize / host_get_secret_slot / WASM blob store / chat e2e bench+chaos / playground frontend）
  - 测试基线表（providers 143 / server 50 / wasm 18 / storage 30 / web 100+）
- tests badge 498/93 → 549/100+

### Why

第一刀 README 写"第一刀打磨完成"是粉饰——followup 揭出 6 类问题。本步重写让 README 反映**双刀真实状态**：做了多少 + 哪些是文档 + 真实债务在哪。新读者一眼看到这是"打磨中"而非"已完成"。

---

## [0.4.118] — 2026-05-26

**Type**: docs · **主题**：product-gaps.md 加第二刀完成项汇总 + 诚实评。

### Changed

- `docs/product-gaps.md` 章节"已收口"重写：
  - 第一刀（0.4.65-0.4.101，37 patch）+ 第二刀（0.4.102-0.4.117，16 patch）= 总 53 patch
  - 第一刀表保留 8 项核心 + 5 项关键阶段
  - 第二刀新表 16 项，每项标 followup 章节号 + Type（runtime/refactor/test/design/docs）
  - "诚实评"段：真改 runtime 5 项 / 设计稿+文档+测试 11 项 / 撤回误判 1 项 / 粉饰更正
  - "真实债务推 v0.5.x" 8 项明示

### Why

第一刀的 product-gaps 表只列了 8 项 ✓ 全打；followup 揭出来的 6 类粉饰需要在 product-gaps 中也体现，不能仅藏在 followup.md。让任何运维 / 用户读 product-gaps 一眼看到"做了什么 + 哪些是文档 + 真实债务在哪"。

---

## [0.4.117] — 2026-05-26

**Type**: docs · **主题**：SECURITY.md 完整化（followup §5.2）。

### Changed

- `SECURITY.md` 从 42 行扩到 ~120 行，加 7 个段：
  - **Supported Versions**：0.4.x active / 0.3.x security-only / ≤0.2.x EOL，明示 v0.5.0-rc1 发布后 0.3.x EOL 时间线
  - **Reporting a Vulnerability**：GitHub Security Advisory（推荐） / Email / 紧急情况 3 渠道 + 6 项必填内容
  - **Response SLA**：72h acknowledgement / 7d triage / 14d-90d fix（按严重度）/ fix+7d coordinated disclosure 四阶段
  - **Coordinated Disclosure**：4 步流程 + 拒绝勒索式威胁但允许 90d 长期未响应后报告者公开
  - **高风险类别**：P0 (secret 泄露 / tenant 越权 / admin takeover / SSRF) / P1 (billing 绕过 / JWT 固化 / upstream body 泄漏) / P2 (rate limit 绕过 / WASM 资源耗尽 / audit 完整性) 三级
  - **Security Advisory 历史**：链向 GitHub advisories
  - **NOT a vulnerability**：6 类不算安全问题，走 issues

### Why

第一刀 followup §5.2 揭：SECURITY.md 42 行简陋，缺 SLA / disclosure timeline / contact channel / severity tiers。开源项目 GitHub 期望规范 SECURITY.md（advisory 系统集成 + 报告者预期管理）。

诚实评：response SLA 是承诺，实际执行能力取决于 maintainer bandwidth；先写出来作为目标。

---

## [0.4.116] — 2026-05-26

**Type**: docs · **主题**：admin.rs 拆分进度表 + ROADMAP 第二刀分类汇总。

### Changed

- `crates/gate-server/src/routes/admin.rs` 头部加 "模块拆分进度（B1）" 段：
  - 7 个业务块（channels / users / sso / groups / org_members / invitations / probe / pricing）行范围 + 状态表
  - 当前 ✅ 拆出 2 个：`mod pricing`（0.4.72）+ `mod org_members`（0.4.109）
  - ⛔ 仍顶层 5 个，每个标注"为什么没拆"（god-tier / 跨 fn 共享 helper / 内聚密）
  - 真拆物理文件计划推到 v0.5.x，列出目标目录结构

- `ROADMAP.md` 加"第二刀（0.4.102-0.4.120）"段：
  - 真改 runtime 5 项 ticked
  - 文档 + 测试 + 设计稿其余 9 项 ticked
  - 剩余真重构 8 项明示推到 v0.5.x（含 admin 真拆 / channels store / DataTable / host_get_secret_slot / WASM blob store / chat bench / chaos test / playground frontend）

### Why

第一刀 followup §1 揭"step 1/4"假象。本步把"做到哪、剩多少、为何没继续"全摊到 admin.rs 文件头 + ROADMAP，让任何接手者一眼看到真实进度——不再粉饰成"已拆"。

诚实评：仍是文档版本。admin.rs 行数没减，但**心里没鬼**。

---

## [0.4.115] — 2026-05-26

**Type**: design · **主题**：DataTable virtualization 完整设计稿（B4 step 2-3 真图）。

### Added

- `docs/data-table-virtualize-design.md`（~150 行）：
  - 现状：0.4.85 仅加 maxHeight + stickyHead，DOM 仍渲染所有 row
  - v1 接口契约（rows + rowSnippet + rowHeight + overscan）
  - Layout 算法（spacer tr + slice，等高假设）
  - 性能预算表（10k rows: legacy 3s 卡死 → virtualize <50ms 首屏 60fps）
  - Caller migration（admin/requests / audit / incidents / groups）
  - 已知限制 4 项 + 验收门禁 5 项 + 不做什么 4 项

### Why

第一刀 followup §1：B4 写 "step 1/3"，让人以为后续会做 step 2/3，但实际就停在 sticky head。本设计稿真正给 step 2/3 画图：

- 锁 v1 接口（rowHeight 等高 + slice 渲染 + spacer tr）
- 算法极简（避免 ResizeObserver / IntersectionObserver 复杂度）
- legacy mode 保留（百行 / 数十行 caller 0 改动）

### Honest assessment

诚实评：DataTable.svelte 仍只 60 行，没接受 rows prop。本步是文档钉死方案。v0.5.x 实装时按本设计 PR review 即可。

---

## [0.4.114] — 2026-05-26

**Type**: test · **主题**：channels form-factories 4 个 sanity test。

### Added

- `web/src/tests/channels-form-factories.test.ts`：
  - defaultCreateForm 返业务约定默认值（timeout_ms=60000 / max_retries=2 等）
  - 每次调用返新 object（array / nested object 也不共享 ref，防 mutate 污染）
  - defaultEditForm 返空对象 + 每次新对象

### Why

第一刀 followup B2 真做 step 3（list state store）涉及 page state 全重构，工作量超 patch。先用 test 锁死 0.4.110 form-factories 的业务契约——任何 default 值漂移（如未来人手贱把 max_retries 改成 3）会立即被 CI 拦截。

### Honest assessment

诚实评：B2 step 3 (list state store) 真重构还没做。本步只是把 step 2 的工厂加测试。

### Verification

```bash
npm --prefix web test -- channels-form-factories    # 4 passed
```

---

## [0.4.113] — 2026-05-26

**Type**: docs · **主题**：撤回 product-review §1 P1-3 误判（request_logs 已是 outbox 异步）。

### Changed

- `docs/product-review-followup-2026-05-26.md` 末尾加 "0.4.113 误判更正" 段：
  - 复审发现 `RequestLogRepo` trait 实际只读（list/find/stats/partition 管理，无 insert/write）
  - 真实架构：`request_events` canonical 主表（outbox 路径）+ `request_log_events` 月度分区 read 投影
  - billing outbox consumer 在 worker plane 异步 batch 写
  - 撤回 0.4.97 占位 env (`KOOIX_REQUEST_LOG_BUFFER_SIZE` 等) 的"必要性"——除非未来要给 read 投影加缓冲，但读路径已是异步

### Why

第一刀 review 把 `RequestLogRepo` 看成同步 writer 是望文生义。`grep INSERT.*request_log` 命中 0 即证据：根本没有同步 INSERT 路径。

诚实更正比悄悄忽略好——既然 followup 是批判稿，自审错误也要写进去。

---

## [0.4.112] — 2026-05-26

**Type**: docs · **主题**：Grafana dashboard 修指标名漂移 + 加 chat panel（followup §5.1）。

### Changed

- `deploy/grafana/dashboards/kooix-gate-overview.json`：
  - 🩸 **修真实 bug**：原 `gate_chat_latency_ms_bucket` **指标根本不存在**（0.4.66 真名是 `gate_chat_duration_seconds`）—— p95 Latency panel 一直在拉空数据
  - 新增 4 个 panel：
    - **p95 TTFB (streaming)** stat
    - **Chat duration p50/p95/p99 by streaming** timeseries（用 streaming label 区分流/非流，避免长流污染 p99）
    - **Chat error rate by outcome** timeseries（按 provider_type + streaming 切片）
    - **SSE chunks/s by model** timeseries
  - 全部用 0.4.66 实装的真实 metric 名

### Why

第一刀 followup §5.1：observability.md 写了新指标但 dashboard 没补。本版本一次性修旧 bug + 加新 panel。

### Verification

```bash
node -e "JSON.parse(require('fs').readFileSync('deploy/grafana/dashboards/kooix-gate-overview.json'))"   # JSON valid
```

运维拉新 dashboard 后，原 p95 Latency stat 会从"空"变成"真实曲线"。

---

## [0.4.111] — 2026-05-26

**Type**: design · **主题**：`host_get_secret_slot` 完整设计稿（B3a step 3/3，G-003 收尾）。

### Added

- `docs/wasm-secret-slot-design.md`（设计稿，~140 行）：
  - 必要性（plugin 在 transform hook 拿 secret 用于 Auth header / 解密 / HMAC）
  - ABI 函数签名（4 参数 i32 + i32 错误码）
  - 6 个错误码（成功 / 空 / -1 not_allowed / -2 missing / -3 too_small / -4 invalid_name / -5 host_error）
  - Audit 半生命周期（每次调用 emit + 60s sliding window 节流防风暴）
  - Capability 校验（manifest `security.permissions.secret_slots` 在 load_module 时存到 `ChannelModule.allowed_slots`）
  - Host context 传递（HookContext 加 `secrets: HashMap<String, String>`，调用方解密后过滤好再塞）
  - Linker 注册示例 + Rust SDK 包装代码
  - 5 项验收门禁
  - 4 个"不做什么"边界

### Why

第一刀 followup §1：把 host_get_secret_slot 推到"下版本"是粉饰。这是 G-003 三件套的第三件，缺它插件无法做最常见的 secret 操作。本版本不实装（涉及 HookContext schema 改 + WasmtimeHost data type 替换 + 4 个新 metric + audit 链路，工作量超 patch），但**把方案完全钉死**让 v0.5.x 实装时不需要再讨论 ABI 细节。

### Honest assessment

诚实评：仍是文档版本。host_get_secret_slot 在 codebase 中仍**完全不存在**。

---

## [0.4.110] — 2026-05-26

**Type**: refactor · **主题**：channels page B2 step 2 — createForm/editForm 工厂抽 `_lib`（followup §1）。

### Added

- `web/src/routes/channels/_lib/form-factories.ts`：
  - `defaultCreateForm(): CreateChannelRequest` — 11 字段 + 显式类型保护
  - `defaultEditForm(): UpdateChannelRequest` — 空对象工厂

### Changed

- `web/src/routes/channels/+page.svelte`：
  - 初始化 `createForm = $state(defaultCreateForm())` 替换 inline 11 字段对象字面
  - reset 路径（line 770）改 `createForm = defaultCreateForm()`，避免与初始化漂移
  - editForm 同理

### Why

第一刀 followup §1：B2 step 1（plugin samples）只动了静态文本。本步真改 form 默认值散在两处的问题——之前 inline 字面在 `$state` 初始化和 reset 两处各写一次，任何字段变更要改两处，未来加 `health_check_url` 等字段容易漏。

### Verification

```bash
npm --prefix web run check    # 0 errors / 0 warnings
```

---

## [0.4.109] — 2026-05-26

**Type**: refactor · **主题**：admin.rs B1 step 2/4 — org members 块封装内联 mod（followup §4）。

### Changed

- `crates/gate-server/src/routes/admin.rs` 把 `list_org_members` / `add_org_member` / `remove_org_member_handler` 三个 handler 搬到 `mod org_members { use super::*; ... }` 内联子模块，全部 `pub(super)`
- 主 router 内 2 处 `.route()` 改用 `org_members::list_org_members` / `org_members::add_org_member` / `org_members::remove_org_member_handler`

### Why

跟 v0.4.72（pricing）同套路。org members 是 invitations 大块（17 fn）中**最独立的子块**：3 个 handler 都只调 `app.repos.memberships.*` repo 方法，不依赖 invitations 内部 helper。

诚实评：admin.rs 4248 → 4252 行（+4，pub(super) 注解 + mod 包裹）。继续是"逻辑边界封装"而非物理拆分；剩下 invitations 块（14 fn）含 `create_invitation` / `revoke_invitation` 跨多 handler 共享，且与 `invitation_token_hash` / `invitation_accept_url` helper 紧耦合，本 patch 不动。下一步：v0.4.116 groups / v0.4.117 sso。

### Verification

```bash
cargo check -p gate-server         # 0 errors
cargo test -p gate-server --lib    # 50 passed (无回归)
```

---

## [0.4.108] — 2026-05-26

**Type**: docs · **主题**：chat 流式语义注释 + stream_safe 用法范例（followup §6）。

### Changed

- `crates/gate-server/src/routes/chat.rs` 流式分支加 8 行注释：
  - 明示"流式建立失败不 retry"（语义等价 RetryConfig::stream_safe）
  - 解释为何**不调** with_retry：客户端已收 chunk + inflight pre-debit 已扣
  - retry_config 仅在非流分支（line ~422）生效
- `crates/gate-providers/src/retry.rs::RetryConfig::stream_safe` doc-comment 加 `# 推荐用法` ignore-block 范例

### Why

第一刀的 followup §6 揭：`stream_safe()` 是幽灵 API（codebase 零调用）。第一种修法是删掉 API（最干净），第二种是真用。考虑到 chat handler 的"流式不调 with_retry"是合理设计（不需要 retry wrapper 的 overhead），保留 API + 在源码里**用注释把语义钉死**——让 future maintainer 看到"为什么流式没 retry"立刻明白。

诚实评：这版本只是文档化，**没改 runtime 行为**。stream_safe 仍是 0 个业务调用。下一刀如果加 `with_retry(&stream_safe(), || provider.chat_stream(req))` 把 wrapper 加上才算真用。

---

## [0.4.107] — 2026-05-26

**Type**: refactor · **主题**：metric 名抽 `pub mod names` const（followup §3.5）。

### Added

- `crates/gate-server/src/metrics.rs::names` 新模块，21 个 const：
  - 4 chat metric: `CHAT_REQUESTS_TOTAL` / `CHAT_DURATION_SECONDS` / `CHAT_TTFB_SECONDS` / `CHAT_STREAM_CHUNKS_TOTAL`
  - 7 HTTP lifecycle: requests_total / duration / tokens / active_requests 等
  - 2 upstream + 4 provider routing + 5 quota/billing + 1 worker

### Changed

- `record_chat_request` / `record_chat_ttfb` / `record_chat_stream_chunks` 内部 `metrics::counter!("gate_chat_requests_total", ...)` 改为 `metrics::counter!(names::CHAT_REQUESTS_TOTAL, ...)`，让 typo 在编译期暴露。

### Why

第一刀的 metric 名字符串散在 metrics.rs 闭包内、observability.md 表格、Grafana dashboard JSON 三处。任何 typo 只能 PR review / 抓 bug 时发现。

下一步（v0.4.112 Grafana / 0.4.119 README）可以引用同一 const，避免文档漂移。

### Verification

```bash
cargo test -p gate-server --lib    # 50 passed (无回归)
```

---

## [0.4.106] — 2026-05-26

**Type**: test · **主题**：chat.rs 埋点编译期 grep 验证（followup §3.4）。

### Added

- `crates/gate-server/src/routes/chat.rs` 末尾 `mod metrics_callsite_tests` 4 个 test：
  - `record_chat_request_has_four_callsites` — 4 个出口（流式 build err / 流式 trigger / 非流 Err / 非流 Ok）
  - `record_chat_ttfb_has_one_callsite` — 流式首 chunk inspect
  - `record_chat_stream_chunks_has_one_callsite` — 流式 trigger 收尾
  - `streaming_branch_emits_both_ok_and_error_outcome` — 流式 ok/error 都 emit
- 用 `include_str!("chat.rs")` 把源码作为字符串扫描

### Why

第一刀只在 metrics.rs 加了 emit 函数的单测（验证函数本身能写入 prometheus），**没有验证 chat handler 真调了**这些函数。未来 refactor 误删埋点 CI 不会失败。

不写真 e2e（需 axum + auth + mock provider，>200 行 fixture），用 grep test 廉价覆盖。

### Verification

```bash
cargo test -p gate-server --lib metrics_callsite    # 4 passed
```

---

## [0.4.105] — 2026-05-26

**Type**: runtime · **主题**：SharedHttpClient eviction 从 clear-all 改 LRU per-key（followup §3.3）。

### Changed

- `SHARED_CLIENT_CACHE_LIMIT`: 8 → 16（给 plugin manifest custom timeout 留余量）
- 缓存 entry 新增 `last_used: Instant`，每次 hit 刷新
- 超限 eviction 策略：从 `cache.clear()` 改为 `min_by_key(last_used)` 删一个最久未用 entry
- 测试 3 个合并到一个 `#[test]`（避免 cargo test 并发跑共享 cache 互相干扰）

### Why

第一刀的 `cache.clear()` 是图省事——一旦 `cache.len() ≥ 8` 就**清空所有 client**。如果有 9 个不同 timeout 桶（plugin manifest `request.timeout_ms` 自定义会扩散维度），**每来一个新 timeout 都触发全 cache 清空** → 所有 channel 重连，雷暴。

LRU per-key 只删一个，其余 entry 的 keep-alive 连接保留。

### Verification

```bash
cargo test -p gate-providers --lib    # 143 passed
```

新合并 test `shared_clients_full_behavior`：
1. same opts → 同 Arc
2. different opts → 不同 Arc
3. 填满 16 个 + 访问 idx=1 让它 MRU + 触发 evict + 验证 idx=0 被删 + 验证 idx=5 没被删

---

## [0.4.104] — 2026-05-26

**Type**: runtime · **主题**：Usage 加 audio/prediction tokens（followup §3.2）。

### Added

- `Usage.audio_tokens: Option<u32>` — OpenAI 4o-realtime / o1-audio
- `Usage.accepted_prediction_tokens: Option<u32>` — predicted outputs 被接受的 token 数
- `Usage.rejected_prediction_tokens: Option<u32>` — predicted outputs 被拒绝的 token 数

### Changed

- `lift_openai_usage_details` 重写借用模式（先 copy 出 5 个 nested 字段值再统一调 entry，避免 immutable/mutable borrow 跨调用冲突）
- 5 个字段全部接入 lift 路径
- `plugin_preset.rs` / `custom_provider/replay.rs` Usage struct literal 补 3 个新字段默认值

### Why

第一刀只 lift 了 `cached_tokens` + `reasoning_tokens` —— 4o-realtime / o1-audio 模型的 audio_tokens 和 predicted outputs 系列模型的 accepted/rejected 完全丢失，billing 拿不到完整维度（accepted 按正常 token 计费，rejected 通常折扣或免费）。

### Verification

```bash
cargo test -p gate-providers --lib    # 144 passed (143 + 1 lift_extracts_audio_and_prediction_tokens)
```

---

## [0.4.103] — 2026-05-26

**Type**: runtime · **主题**：Retry-After 头兼容 HTTP-date 格式（followup §3.1）。

### Added

- `retry.rs::parse_retry_after(value: &str) -> Option<u64>` — RFC 7231 §7.1.3 兼容解析：
  - delta-seconds 数字：原样保留 + ×1000 转毫秒
  - HTTP-date IMF-fixdate（用 `chrono::DateTime::parse_from_rfc2822`）：算到当前 Utc 的差值
  - HTTP-date 已过期：返回 `Some(0)` 告诉调用方"无需等"
  - 解析失败：`None`

### Changed

- `crates/gate-providers/src/openai.rs::check_status` 与 `anthropic.rs::check_status` 都改用 `parse_retry_after`，移除原 `parse::<u64>().map(|s| s * 1000)`。

### Why

云厂商（如 Cloudflare 中间层、AWS API GW）在限流响应中**经常用 HTTP-date 格式 Retry-After**。原实现对此 fall-through 成 None → 用默认 backoff 重试，可能比上游期望更早，造成二次冲击。

### Verification

```bash
cargo test -p gate-providers --lib retry::    # 9 passed (5 既有 + 4 新)
```

新测试：
- `parse_retry_after_delta_seconds` — 数字 / 0 / 含空白
- `parse_retry_after_http_date_future` — IMF-fixdate 未来 30s 解析
- `parse_retry_after_http_date_past_returns_zero` — 过期返回 0
- `parse_retry_after_garbage_returns_none` — 空 / 非数字 / 浮点 / 负数

---

## [0.4.102] — 2026-05-26 — 第二刀启动 · followup 批判稿

**Type**: docs · **主题**：自我批判第一刀 37 patch，揭"伪完成"。

### Added

- `docs/product-review-followup-2026-05-26.md`（230+ 行）：
  - 第一类：假步骤命名（admin.rs / channels page / WASM 三件套 / DataTable 实际只做 step 1，CHANGELOG 写 step 1/N 误导）
  - 第二类：占位算实装（KOOIX_REQUEST_LOG_BUFFER_SIZE / chat e2e bench / chaos-testing.md / playground capability 都是文档/TODO 而非 runtime）
  - 第三类：漏网（Retry-After HTTP-date / lift_openai_usage_details 缺 audio + prediction tokens / SharedClient LRU 雷暴 / chat metrics 埋点未 e2e 验证 / metric name 散在多处）
  - 第四类：内联 mod 是假拆分（admin.rs +13 行）
  - 第五类：文档残留漂移（Grafana dashboard / SECURITY.md / RELEASE.md 检视表粉饰）
  - 第六类：stream_safe 是幽灵 API（零业务调用）
  - 第二刀路线：v0.4.103-0.4.120 按 P0/P1/P2 映射 18 个修复 patch

### Why

CHANGELOG 把所有 37 patch 写成"已收口"是粉饰：真改 runtime 的约 15 个，其余是文档/测试/sanity/占位/口号。本版本不修代码，只把"什么是 真实装 / 什么是 文档 / 什么没做"摆到桌面上。诚实优先。

### 自审重点

- "step 1/N" 命名误导：admin.rs 4 步只做 1 步、channels page 4 步只做 1 步、WASM 3 步只做 2 步、DataTable 3 步只做 1 步
- 占位 env 与 TODO 注释被列入 CHANGELOG 主路径
- 内联 mod 使 admin.rs 行数 +13 还自称"逻辑边界清晰"
- `RetryConfig::stream_safe()` 整个 codebase 零调用
- Retry-After HTTP-date 格式漏解析（RFC 7231 必修）
- OpenAI o1 系列 audio_tokens / prediction_tokens 漏 lift

---

## [0.4.101] — 2026-05-26

**主题**：「空衍」logo 重新设计 — 中心负空间方框 + 4 螺旋臂 + 角点 + 灵气短戟。

### Changed

- `web/scripts/generate-kooix-logo.mjs` 重写设计语言（D4 静态对称 → 风车螺旋）：
  - **空**：中心同心方框（38×38 + 22×22 rounded square），currentColor stroke + 16% opacity fill，表达「路由门户 / 负空间」
  - **衍**：4 个螺旋臂，起 r=26 终 r=86 sweep=95°，Catmull-Rom 平滑成 cubic bezier，每臂末端缀 token 圆点。-8° 起始偏移营造逆时针风车动感
  - **栅**：对角线 4 主圆角方 (r=104) + 4 副小圆点 (r=118)
  - **气**：4 个主基本方向 8 段虚线短戟（近 stroke=2.6 远 stroke=2.2）
- 几何参数预先用 `ctx_execute` 跑通：相邻臂端起距 60px ≥ 50px 不缠绕，端点离 viewBox 边 ≥ 42px 安全
- `KooixLogo.svelte` 兼容旧 caller：保留 `tone='mark' | 'tile'` prop，'tile' 模式带 zinc 圆角方块底（用于 Sidebar）
- favicon (64×64) 同步简化版：去 aura + 短螺旋臂 (sweep=85°, 直线段) + 中心方框

### Why

原 logo 是 D4 90° 旋转对称的四角星 + 4 条飘带 + 12 点节点，**克制有余、灵动不足**——更像"佛印"而非"衍"。「空衍」的语义需要：

- **空**：留白作为主角（不是装饰）— 改用中心方框的"门"
- **衍**：演化、推衍、流动 — 改用螺旋而非对称飘带
- 仍保留 D4 对称作为骨架，让识别度不变

### Verification

```bash
node web/scripts/generate-kooix-logo.mjs   # 3 文件生成
npm --prefix web run check                  # 0 errors / 0 warnings
```

### 视觉对比

| 项 | v1 (0.4.x baseline) | v2 (0.4.101) |
|----|---------------------|--------------|
| 中心 | 实心四角星 | 同心负空间方框 |
| 主元素 | 4 条对称飘带 | 4 个螺旋臂（风车感） |
| 节点 | 4 主 + 8 副粒子 | 4 主圆角方 + 4 副圆点 |
| 装饰 | 4 条点画曲线 | 8 段虚线灵气短戟 |
| 动势 | 静态对称 | 逆时针风车螺旋 |
| 寓意 | 几何均衡 | 空（路由门）+ 衍（token 推演） |

---

## [0.4.100] — 2026-05-26 — 阶段大版 · product-review 第一刀收口

> 36 个 patch（0.4.65 → 0.4.100）完整覆盖 [product-review-2026-05-26.md](./docs/product-review-2026-05-26.md) 第一刀。
> v0.5.0-rc1 候选门禁见 [RELEASE.md § rc1 准备清单](./RELEASE.md)。

### 阶段战报

| 域 | 完成项 | 关联 patch |
|----|------|-----------|
| **性能** | SharedHttpClient 4 fast-path 共享 reqwest pool + hit/miss/evict/size 指标 | 0.4.65 / 0.4.94 |
| **可观测** | `gate_chat_*` 4 指标（duration/ttfb/stream_chunks/requests_total）+ WASM 9 指标 + observability.md 完整对齐 | 0.4.66 / 0.4.73 / 0.4.95 |
| **渠道一致性** | Anthropic/Bedrock 透传 ChatRequest.extra；OpenAI/Azure nested usage details auto-lift | 0.4.67 / 0.4.75 |
| **Usage 字段** | cache_creation_input_tokens；o1/o3 reasoning_tokens/cached_tokens 自动提升 | 0.4.68 |
| **安全** | ProviderError body 脱敏（512B + sha256 + UTF-8 边界感知）+ runbook + threat-model | 0.4.69 / 0.4.92 / 0.4.93 |
| **可靠性** | Retry ±25% jitter + `RetryConfig::stream_safe()` factory | 0.4.70 |
| **配置** | PgPool 5 env 显式化 + `.env.example` 4 段补全 | 0.4.71 / 0.4.74 / 0.4.97 |
| **重构** | admin.rs pricing 内联 mod；channels page plugin samples 抽 `_lib` + 6 测试 | 0.4.72 / 0.4.76 / 0.4.77 |
| **WASM host functions** | host_log 真实实装 + host_record_metric sanitize（B3a） | 0.4.80 / 0.4.81 / 0.4.82 |
| **WASM cache** | cwasm 持久化（`KOOIX_WASM_CACHE_DIR` env + Module::deserialize_file + 4 指标 + runbook） | 0.4.83 / 0.4.84 / 0.4.89 / 0.4.96 |
| **DataTable** | maxHeight + stickyHead + 3 sanity tests | 0.4.85 / 0.4.86 |
| **Provider capabilities** | `GET /v1/admin/providers/capabilities` 11 项矩阵 + endpoint sanity test | 0.4.87 / 0.4.88 |
| **文档** | RELEASE.md rc1 清单 / ROADMAP / product-gaps / playground / README badges / chaos-testing.md 设计稿 | 0.4.73 / 0.4.78 / 0.4.79 / 0.4.90 / 0.4.91 / 0.4.99 |
| **bench TODO** | hot_paths.rs 加 chat e2e bench 实施 TODO（0.5.x 实装） | 0.4.98 |

### 验证

```bash
cargo check --workspace                                       # 0 errors
cargo test -p gate-providers --lib                            # 139 passed (从 122 增 +17)
cargo test -p gate-server --lib                               # 46 passed (从 41 增 +5)
cargo test -p gate-wasm --lib                                 # 18 passed (从 13 增 +5)
cargo test -p gate-storage --lib pool_config_tests            # 5 passed (新增)
npm --prefix web run check                                    # 0 errors / 0 warnings
npm --prefix web test                                         # 93 web tests (从 87 增 +6)
```

### 阶段亮点

- **零回归**：36 个 patch 全部 cargo check + 涉及 crate 测试通过；前端 0/0 维持。
- **每 patch 独立可逆**：commit 粒度细，git revert 单个 patch 不影响其他。
- **CHANGELOG 完整**：每个 patch 都写 主题 / Changed / Why / Verification 四段，回看 36 个 commit 清晰可读。
- **文档与代码同步**：observability / ROADMAP / product-gaps / RELEASE / playground / wasm-runbook / security-runbook / threat-model / chaos-testing 9 个文档全部对齐 0.4.65-0.4.99 实装。

### 下一步：v0.5.0-rc1

按 [RELEASE.md § rc1 准备清单](./RELEASE.md#v050-rc1-准备清单基于-product-review-2026-05-26) 跑候选门禁。剩余 product-review P1/P2 项（playground frontend / channels page deep 拆 / request_logs buffered runtime / chat e2e bench 真实装 / chaos test runtime / host_get_secret_slot）进入 v0.5.x 迭代。

---

## [0.4.99] — 2026-05-26

**主题**：`docs/chaos-testing.md` 设计稿（0.5.x 实装路径文档化）。

### Added

- 新增 `docs/chaos-testing.md`：
  - 目标 / 不做什么
  - Phase 1 故障矩阵：27 个 case（PG 9 / Redis 6 / 上游 12）含工具 + 期望行为
  - Phase 2 自动化（`tests/chaos/` 目录 + `make chaos` target + metric 断言）
  - Phase 3 drill-friendly fixtures（with_pg_latency helper / blast radius 注释）
  - Coverage targets + 关联文档

### Why

product-review §5.2 判词："缺 deterministic 复现 case，限流挂掉 / Redis 闪断 / 上游 503 风暴 / pool 耗尽只能事后复盘"。本版本不实装（涉及 toxiproxy 容器 + 多 case，工作量超 patch 范围），先把方案锁定让 0.5.x 实装时有图纸。

---

## [0.4.98] — 2026-05-26

**主题**：hot_paths.rs 顶部加 chat e2e bench 实施 TODO（0.5.x 实装方向）。

### Docs

- `crates/gate-server/benches/hot_paths.rs` 顶部 doc-comment 加 4 行 TODO 块：
  - 当前覆盖范围（quota / billing 微观路径）
  - 缺口（"request 进 axum → response 出 axum" 端到端 latency）
  - 0.5.x 实装方向 4 步（wiremock mock 上游 / reqwest 打内部 router / criterion group chat_e2e + chat_stream_e2e / baseline JSON 入 bench/results/）
  - 与 plugin_vs_builtin bench 区别说明

### Why

product-review §5.2 列 "chat e2e bench 缺" 为 P1 项。本版本只锁 API 形状与实施路径，避免后续 refactor 时方向漂移。

---

## [0.4.97] — 2026-05-26

**主题**：`.env.example` 加 request log buffered writer 占位 env（0.5.x 实装前文档化）。

### Docs

- `.env.example` 新增段：
  - `KOOIX_REQUEST_LOG_BUFFER_SIZE` — 单 batch 上限（默认 512）
  - `KOOIX_REQUEST_LOG_FLUSH_INTERVAL_MS` — 强制 flush 间隔（默认 200ms）
  - `KOOIX_REQUEST_LOG_BACKPRESSURE` — block / drop_oldest / drop_newest 三种背压策略

### Why

product-review §1 P1-3：`request_logs` 写入热路径同步 insert，写放大严重。本版本先文档化 env 接口，runtime 实装到 0.5.x（涉及 PgRequestLogRepo + 后台 flush 任务 + 错误恢复，不在本 sprint 范围）。

文档化锁住 API 形状，让运维提前规划部署参数；同时明示"未实装"避免 runtime 误读。

---

## [0.4.96] — 2026-05-26

**主题**：.gitignore 排除 cwasm 缓存（0.4.83 配套）。

### Changed

- `.gitignore` 加 `.wasm-cache/` 与 `*.cwasm` —— 本地开发若 `KOOIX_WASM_CACHE_DIR` 不慎指到 repo 内，不会污染 `git status`。

---

## [0.4.95] — 2026-05-26

**主题**：observability.md 补 0.4.80-0.4.94 新增 9 个指标。

### Docs

- `docs/observability.md` 三个 section 扩充：
  - `WASM Plugin` 补 `gate_plugin_wasm_host_log_total{level}` + 4 个 cwasm cache 指标（hit/miss/corrupt/write）
  - 新增 `Plugin user metrics` 段：`plugin_wasm_user_*` namespace（来自 host_record_metric）
  - 新增 `Upstream HTTP client` 段：4 个 SharedHttpClient 指标（hits/misses/evictions/size）

### Why

0.4.80 host_log / 0.4.81 host_record_metric / 0.4.83 cwasm cache / 0.4.94 SharedClient 共加 9 个新指标但都没进 observability 表格 —— 运维拿表当 dashboard 工程蓝图，遗漏即变 silent metric。

---

## [0.4.94] — 2026-05-26

**主题**：SharedHttpClient 加 hit/miss/evict/size 指标（0.4.65 配套可观测）。

### Added

- `crates/gate-providers/src/lib.rs::shared_http_client` 三处 emit：
  - cache hit → `gate_providers_shared_client_hits_total`
  - eviction → `gate_providers_shared_client_evictions_total`（LRU 满 8 时 clear all）
  - miss + insert → `gate_providers_shared_client_misses_total` + gauge `gate_providers_shared_client_size`
- `gate-providers/Cargo.toml` 加 `metrics = { workspace = true }` 依赖

### Why

0.4.65 实装 SharedHttpClient 时只暴露 cache 行为，没有指标。运营时无法知道 cache 是否在工作（hit 率高低 / 是否频繁 evict 触发重连）。补足这 4 个指标让 LRU 容量调优有数据支撑。

### Verification

```bash
cargo check -p gate-providers          # 0 errors
cargo test -p gate-providers --lib     # 139 passed (无回归)
```

---

## [0.4.93] — 2026-05-26

**主题**：threat-model.md 加 "Upstream error body leakage" 威胁条目（0.4.69 STRIDE 文档化）。

### Docs

- `docs/threat-model.md` 新增第 7 个 STRIDE 条目：
  - Threats: 上游回显 PII / key；长 body 放大日志压力
  - Controls: `ProviderError::upstream` 工厂 + 512B 截断 + sha256 哈希；audit_redaction 链
  - Verification: `error::tests` 4 case + manual 4xx body 验证

### Why

0.4.69 实装代码 + 0.4.92 写 runbook 步骤，但还差正式威胁建模条目。补齐 STRIDE 链路。

---

## [0.4.92] — 2026-05-26

**主题**：security-runbook 加 "Provider 上游 error body 泄漏" 段（0.4.69 配套）。

### Docs

- `docs/security-runbook.md` 新增段，覆盖：
  - 0.4.69 redact_upstream_body 行为（≤512 原样 / >512 截断 + sha256）
  - UTF-8 边界感知
  - 必修原因（上游回显 PII / key 风险）
  - 排查 3 项：构造点是否走 `ProviderError::upstream` 工厂 / 自定义 provider 是否绕过 / audit_redaction 是否禁用

---

## [0.4.91] — 2026-05-26

**主题**：ROADMAP.md 加 product-review 第一刀完成项段（0.4.65-0.4.90 已 ticked 14 条）。

### Docs

- `ROADMAP.md` § M3 后新增 "M3 后 — product-review 第一刀" 章节：
  - 14 条已完成项 ticked，按性能/可观测/渠道/Usage/安全/可靠/配置/重构/WASM/前端/能力面/文档分类
  - 与 [product-review-2026-05-26.md](./docs/product-review-2026-05-26.md) / [product-gaps.md](./docs/product-gaps.md) 三向引用

---

## [0.4.90] — 2026-05-26

**主题**：playground.md M1.5 路线第 1 项标 backend 已收口。

### Docs

- `docs/playground.md § 路线（M1.5）` 更新：
  - 第 1 项拆为 Backend / Frontend，Backend 已通（0.4.87 endpoint）
  - Frontend 接入还在路上（FlowEditor 拉 endpoint + store + 节点联动）

### Why

0.4.87 的 `GET /v1/admin/providers/capabilities` 让 playground capability 联动的"数据源"已通。前端接入工作单独迭代。文档同步真相。

---

## [0.4.89] — 2026-05-26

**主题**：wasm-runbook.md 加 cwasm 持久化缓存运维段（0.4.83 配套）。

### Docs

- `docs/wasm-runbook.md` 新增 `## 7. cwasm 持久化缓存`：
  - 启用方式（`KOOIX_WASM_CACHE_DIR`）
  - 路径约定（`{sha256}-wt26-0.cwasm`）
  - 4 个运维要点：wasmtime 升级、wasm 模块更新清理、cache miss 抖动告警、多 replica 共享 PVC

### Why

0.4.83 + 0.4.84 实装了 cwasm cache + env 注入，但 wasm-runbook 没收录运维细节。运维只看 runbook 来调线上，文档闭环。

---

## [0.4.88] — 2026-05-26

**主题**：`provider_capabilities_returns_full_matrix` 单测覆盖 0.4.87 新 endpoint。

### Added

- `crates/gate-server/src/routes/admin.rs::tests::provider_capabilities_returns_full_matrix` — 验证：
  - 至少 5 个 entry（4 编译期 + ≥1 plugin preset）
  - 4 个编译期 provider（openai/anthropic/azure/bedrock）都在 + `kind=compile_time`
  - `plugin:openai_compatible` 存在 + `kind=plugin_preset`
  - `openai.capabilities.chat == true`（基本能力非空）

### Verification

```bash
cargo test -p gate-server --lib    # 46 passed (45 + 1 新增)
```

---

## [0.4.87] — 2026-05-26

**主题**：`GET /v1/admin/providers/capabilities` endpoint（product-review B5）。

### Added

- `crates/gate-server/src/routes/admin.rs::list_provider_capabilities` —— 一次返完整 provider capability 矩阵：
  - 4 个编译期 fast-path provider（openai / anthropic / azure / bedrock）
  - 7 个 plugin preset（openai_compatible / anthropic_messages / google_gemini / cohere / mistral / deepseek / ollama）
- 每条返 `{id, name, capabilities, base_url_hint, kind=compile_time|plugin_preset}`

### Why

product-review §2.3 + G-206：playground 节点联动、channel drawer 默认填值需要按 provider 拿全能力矩阵。之前只能逐 channel 查 `/v1/admin/channels/:id`，前端拉一次 admin endpoint 就能拿到全部，免去 N+1 请求。

### Verification

```bash
cargo check -p gate-server      # 0 errors
cargo test -p gate-server --lib # 45 passed (无回归)
```

---

## [0.4.86] — 2026-05-26

**主题**：DataTable maxHeight / stickyHead 加 3 个 sanity test。

### Added

- `web/src/tests/data-table.test.ts`：
  - 默认无 maxHeight / 不 sticky
  - `maxHeight='480px'` 写入 `style="max-height: 480px; overflow-y: auto"`
  - `stickyHead=true` 单独不渲染 thead（需 head snippet）

### Verification

```bash
npm --prefix web test -- data-table    # 3 passed
```

---

## [0.4.85] — 2026-05-26

**主题**：DataTable.svelte 加 `maxHeight` + `stickyHead` prop（product-review B4 step 1/3，长表头可见）。

### Added

- `web/src/lib/components/templates/DataTable.svelte`：
  - `maxHeight?: string`（默认空 = 无限高）—— 容器纵向滚动上限
  - `stickyHead?: boolean`（默认 false）—— thead `sticky top-0 z-10`，滚动时表头始终可见

### Why

product-review §4.3 判词：admin/requests 大概率万行数据，无虚拟滚动 → 滚到 100 行后表头看不见，体验差。先用 sticky thead + 容器 max-height 解决"看不见列名"的痛点。真正的窗口化虚拟滚动（row recycle）涉及 row renderer 接口重构，留 step 2/3。

零行为变化（默认 prop 全 false / 空），现有调用方不受影响。

### Verification

```bash
npm --prefix web run check     # 0 errors / 0 warnings
```

---

## [0.4.84] — 2026-05-26

**主题**：`WasmHostConfig::from_env()` + `KOOIX_WASM_CACHE_DIR` env 接入。

### Added

- `WasmHostConfig::from_env()` — 读 `KOOIX_WASM_CACHE_DIR`，空字符串 / 未设 → `None`；设置后即 cwasm 缓存目录。
- `.env.example` 新增 WASM Plugin 持久化缓存段，含路径建议（`/var/cache/kooix-gate/wasm`）与文件名约定。

### Why

0.4.83 实装了 cwasm 缓存机制，但要求 caller 显式构造 `WasmHostConfig.cache_dir`。生产场景应该走 env 注入。配合 `.env.example` 让运维一眼可启用。

---

## [0.4.83] — 2026-05-26

**主题**：WASM cwasm 编译产物持久化缓存（product-gaps G-104）。

### Added

- `WasmHostConfig.cache_dir: Option<PathBuf>` — `None` 即旧行为（每次 compile）；`Some(path)` 启用持久化 cache。
- `WasmtimeHost::load_module_with_cache` — 路径 `{cache_dir}/{sha256}-wt26-0.cwasm`：
  - 文件存在 → `Module::deserialize_file` 直接复用编译产物
  - deserialize 失败 → 删除 + fallback compile + warn
  - compile 后 → `module.serialize` + 写盘（写盘失败不阻断 load）
- 3 个新 metric：`gate_wasm_cache_hit_total` / `gate_wasm_cache_miss_total` / `gate_wasm_cache_corrupt_total` / `gate_wasm_cache_write_total`

### Why

product-gaps G-104：之前每次 gate-server 启动都重新 compile 所有 wasm 模块。50 channel × 5 wasm 启动慢；持久化后第二次冷启动直接 deserialize（~ms 级 vs ~100ms compile）。

### Verification

```bash
cargo check -p gate-wasm                                # 0 errors
cargo test -p gate-wasm --lib cwasm_cache               # 1 passed
cargo test -p gate-wasm --lib                           # 18 passed (17 + 1 新)
```

`cwasm_cache_writes_and_hits_on_second_load` 测试：第一次 load 写 cwasm，第二次新 host 实例 + 同 cache_dir 命中（mtime 不变）。

---

## [0.4.82] — 2026-05-26

**主题**：WASM host_record_metric sanitize 规则 4 个 sanity test。

### Added

- `crates/gate-wasm/src/wasmtime_host.rs` 测试模块新增 `sanitize_user_metric_name` 自由函数（与 host fn 闭包内规则等价）+ 4 个 test：
  - 普通名 → `plugin_wasm_user_` 前缀
  - 含特殊字符 → 大写转小写 + 非 [a-z0-9_] 过滤
  - 200 字符长名 → 截至 17 (prefix) + 64 = 81 字符
  - 全特殊字符 / 空 → drop（None）

### Why

host_record_metric 在 0.4.81 真实 linker 挂上了，但闭包内 sanitize 规则没有独立测试覆盖——加 4 个 test 让规则 regression 早暴露。host_get_secret_slot（B3a step 3/3）涉及 manifest secret slot 声明 + audit 链路，推到下一迭代单独处理。

### Verification

```bash
cargo test -p gate-wasm --lib              # 17 passed (13 既有 + 4 新增)
```

---

## [0.4.81] — 2026-05-26

**主题**：WASM `host_record_metric` 实装（B3a step 2/3，G-003 续）。

### Changed

- `crates/gate-wasm/src/wasmtime_host.rs` 加 `host_record_metric(name_ptr, name_len, value_i64)`：
  - 读 wasm memory 取 metric name，按 [a-z0-9_] sanitize + 截断 64 字符
  - 强制前缀 `plugin_wasm_user_` 防止 plugin 污染 gate 内置 namespace
  - 越界保护与 `host_log` 一致
  - emit `metrics::gauge!` 设 value（i64 → f64）
  - name 为空（sanitize 后）时 silently drop

### Why

product-gaps G-003：插件需要把内部状态（如 cache hit 率、自定义计数）暴露给运维。强制前缀 + sanitize 让 plugin 自定义 metric 与 gate 内置指标在 Prometheus 里清晰隔离，运维一眼看到 `plugin_wasm_user_*` 就知道是插件出的。

剩 `host_get_secret_slot` 在 0.4.82 落地（依赖 manifest secret slot 声明，需要更小心的 audit 链路）。

### Verification

```bash
cargo check -p gate-wasm        # 0 errors
cargo test -p gate-wasm         # 13 passed (无回归)
```

---

## [0.4.80] — 2026-05-26

**主题**：WASM `host_log` 真实实装（B3a step 1/3，product-gaps G-003）。

### Changed

- `crates/gate-wasm/src/wasmtime_host.rs::host_log` 从 placeholder 升级为真实实现：
  - 通过 `Caller::get_export("memory")` 拿到 wasm 线性内存
  - 按 (ptr, len) 切片读取 UTF-8 字符串（越界保护：检查 `ptr+len ≤ data.len()` 否则丢弃 + warn）
  - 防 plugin 撑爆日志：单条 ≤ 1KB（超出截断 + `truncated=true` 字段）
  - level 约定：0=trace / 1=debug / 2=info / 3=warn / 4=error，其它走 debug
  - 路由到 `tracing::{trace,debug,info,warn,error}!` + 加 `plugin=true` 字段
  - emit `gate_plugin_wasm_host_log_total{level}` counter

### Why

product-gaps G-003：ABI v0 三个 transform hook 已通，但 `host_log` / `host_get_secret_slot` / `host_record_metric` 都是 placeholder——插件无法记录 log，DX 残废。host_log 是最简单一个，先做。后续 81-82 补 secret_slot 与 metric。

### Verification

```bash
cargo check -p gate-wasm        # 0 errors
cargo test -p gate-wasm         # 13 passed (无回归)
```

---

## [0.4.79] — 2026-05-26

**主题**：README 当前版本段更新到 0.4.78 + tests badge 498/93。

### Docs

- `README.md` "当前版本" 段重写：把 product-review 第一刀 14 个 patch 的成果列出来，原 0.4.60 段降为"基线"小节
- tests badge 485/87 → 498/93（providers 124→139=+15，server 41→45=+4，storage +5，channels samples +6）
- 入口链接指向 `RELEASE.md § rc1 准备清单`

---

## [0.4.78] — 2026-05-26

**主题**：RELEASE.md 加 v0.5.0-rc1 准备清单 + 已完成项检视表。

### Docs

- `RELEASE.md` 文末加 "v0.5.0-rc1 准备清单"：
  - **已完成项检视**（0.4.65-0.4.77）：11 类改动表，含验证数据点
  - **rc1 候选门禁**：fmt / clippy / test / web check / bundle budget / gitleaks 7 条
  - **rc1 验收清单**：6 项 checklist（CHANGELOG / README badge / product-gaps / ROADMAP / docs / tag&push）

### Why

product-review 之后的 11 个 patch 已合 main，但 RELEASE.md 没把"做了什么"与"还差什么"放在一起。rc1 准备清单让发版者一眼看到当前进度与剩余项；门禁清单让 CI/手工跑都有参照。

---

## [0.4.77] — 2026-05-26

**主题**：plugin-samples 加 sanity test（6 case），防止示例文本失效。

### Added

- `web/src/tests/plugin-samples.test.ts`（6 个 case）：
  - `PLUGIN_MANIFEST_EXAMPLE` / `PRIVATE_PLUGIN_MANIFEST_EXAMPLE` / `RESPONSE_SAMPLE_PLACEHOLDER` — JSON 可解析
  - `PROBE_BODY_PLACEHOLDER` — 含 `{{model}}` 占位
  - `PLUGIN_REPLAY_SAMPLE` — 含 SSE `event:`/`data:` 标记
  - `PLUGIN_BUILDER_STEPS` — 长度 7、首尾固定

### Why

0.4.76 把这些常量上提到 `_lib`，但用户复制到 channel manifest 时若字符串里语法错（如缺逗号），编辑器只在用户保存时才报错。加 sanity test 在 CI 早期就拦截示例文本的格式 regression。

### Verification

```bash
npm --prefix web test -- plugin-samples       # 6 passed
```

总测数：87 → 93。

---

## [0.4.76] — 2026-05-26

**主题**：channels page B2 step 1/4 — plugin builder 静态示例文本抽到 `_lib/plugin-samples.ts`。

### Changed

- 新增 `web/src/routes/channels/_lib/plugin-samples.ts`（76 行）：6 个常量 + 1 个类型
  - `PLUGIN_MANIFEST_EXAMPLE` / `PRIVATE_PLUGIN_MANIFEST_EXAMPLE` — manifest 例子
  - `PLUGIN_REPLAY_SAMPLE` — SSE replay 样例
  - `RESPONSE_SAMPLE_PLACEHOLDER` / `PROBE_BODY_PLACEHOLDER` — placeholder 字符串
  - `PLUGIN_BUILDER_STEPS` + `PluginBuilderStep` 类型 — 7 步 builder 标签
- `web/src/routes/channels/+page.svelte` 1252 → 1199 行（-53 / -4.2%）：移除 inline 常量定义，改用 import alias 保持本地变量名不变（最小侵入）

### Why

product-review §4.2 判词：channels/+page.svelte 1252 行 god page 拆分。这是 4 步拆分的第 1 步——先把"零依赖只读静态数据"上提。这部分不涉及 state / 组件协调，搬出来风险最低，验证套路可行后再拆 dialog manager / store。

### Verification

```bash
npm --prefix web run check    # 0 errors / 0 warnings
```

---

## [0.4.75] — 2026-05-26

**主题**：Azure provider 非流路径也走 `lift_openai_usage_details`（一致性补漏，0.4.68 范围扩大）。

### Changed

- `crates/gate-providers/src/azure.rs::chat` 改用 `bytes → Value → lift → ChatResponse` 流程，与 OpenAI provider 对齐。流式路径无需改动（azure.rs 复用 `openai::sse_to_chunks`，已在 0.4.68 接入 lift）。

### Why

Azure OpenAI 与 OpenAI API 协议一致，o1 / o3-mini deployment 同样会回 `prompt_tokens_details.cached_tokens` 与 `completion_tokens_details.reasoning_tokens`。0.4.68 只改了 openai.rs::chat 路径，azure 漏了——同样会丢失 cache/reasoning 计费维度。

### Verification

```bash
cargo check --workspace                       # 0 errors
cargo test -p gate-providers --lib            # 139 passed (无回归)
```

---

## [0.4.74] — 2026-05-26

**主题**：.env.example 补 KOOIX_DB_* + KOOIX_OUTBOX_* 等可调 env 文档化。

### Docs

- `.env.example` 增加两块"可选调优"注释：
  - **PostgreSQL 连接池**：`KOOIX_DB_MAX_CONNECTIONS` / `KOOIX_DB_MIN_CONNECTIONS` / `KOOIX_DB_ACQUIRE_TIMEOUT_SECS` / `KOOIX_DB_IDLE_TIMEOUT_SECS` / `KOOIX_DB_MAX_LIFETIME_SECS`（0.4.71 暴露但 example 漏写）
  - **Worker 节流**：`KOOIX_OUTBOX_BATCH_SIZE` / `KOOIX_OUTBOX_INTERVAL_MS` / `KOOIX_PRICING_SYNC_INTERVAL_SECS` / `KOOIX_INFLIGHT_SWEEP_INTERVAL_SECS`（已实装但 example 缺）

### Why

0.4.71 在代码层暴露了 5 个 `KOOIX_DB_*` env，但 `.env.example` 没同步——运维找不到名字就改不了配置。worker 类 env 同理：observability-runbook 里提到但首次部署的人不会先读 runbook。

把可调项都写到 `.env.example`（注释默认值与含义），新部署用户直接抄即可。

---

## [0.4.73] — 2026-05-26

**主题**：observability.md + product-gaps.md 与 0.4.65-72 实装对齐。

### Docs

- `docs/observability.md § Request lifecycle` 重写：
  - 4 个 chat metric 名/labels 与代码对齐：`gate_chat_requests_total{streaming, outcome}` / `gate_chat_duration_seconds` / `gate_chat_ttfb_seconds` / `gate_chat_stream_chunks_total`
  - 补 `gate_tokens_total` / `gate_request_duration_seconds` / `gate_active_requests` / `gateway_stage_duration_seconds`
  - 加 0.4.66 历史变更说明（旧 `gate_chat_latency_ms` / `gate_chat_tokens` 已合并；旧 dashboard 需更新；streaming 新维度避免长流污染 p99）

- `docs/product-gaps.md` 顶部加"已收口（0.4.65-0.4.72）"章节：8 个 patch 摘要表 + 验证数据，与 [product-review-2026-05-26.md](./product-review-2026-05-26.md) 双向交叉引用。

### Why

product-review 第一刀（A1-A5 + Retry + Pool + admin step 1）8 个 patch 已合 main，但 observability.md 仍写旧指标名（`gate_chat_latency_ms` 不存在 / labels `provider` 应为 `provider_type`）—— 这是文档漂移，运维拿旧名做 dashboard 会扑空。同步修。

### Verification

```bash
grep gate_chat_ docs/observability.md         # 4 occurrences for new names
git log --oneline v0.4.64..HEAD               # 9 commits since pre-review
```

---

## [0.4.72] — 2026-05-26

**主题**：admin.rs B1 step 1/4 — pricing 块封装为内联子模块（product-review §1.4）。

### Changed

- `crates/gate-server/src/routes/admin.rs`：把 `list_pricing_rules` / `upsert_pricing_rule` / `delete_pricing_rule` + 私有类型 `UpsertPricingRuleRequest` + helper `rule_to_row` 全部搬进 `mod pricing { use super::*; ... }` 内联子模块。3 个 handler 标 `pub(super)` 让父 router 引用；`UpsertPricingRuleRequest` 同步 `pub(super)`。
- 主 `router()` 中 `/pricing-rules` / `/pricing-rules/:id` 改用 `pricing::list_pricing_rules` 等限定路径。

### Why

product-review §1.4 判词：admin.rs 4235 行 god file，关注点混杂。这是 4 步拆分的第 1 步，先把最独立的 pricing 块（180 行 / 不依赖其他业务域 helper 除 `audit_meta`/`require_confirmation`/`pricing_rule_audit_snapshot`）封装为内联模块——对外只暴露 3 个 handler，内部类型隐藏。

下一步（v0.4.73-75）同样套路处理 invitations、groups、users，最终物理拆 `admin/{mod.rs, pricing.rs, ...}`。

### Verification

```bash
cargo check -p gate-server                     # 0 errors
cargo test -p gate-server --lib                # 45 passed (无回归)
```

行数变化：admin.rs 4235 行 → 4248 行（+13 行：`mod pricing { use super::*; }` 包裹与 pub(super) 标注）。物理行数小增但逻辑边界清晰：pricing 内部 11 个符号（3 fn + 1 enum + 2 struct + 1 helper）从顶层符号表移入 mod，admin.rs 顶层 fn 数 -3。

---

## [0.4.71] — 2026-05-26

**主题**：PgPool 配置显式化 — `KOOIX_DB_*` env 可调，默认值生产友好（product-review §1.5）。

### Added

- `crates/gate-storage/src/lib.rs::PoolConfig` 结构体：max_connections / min_connections / acquire_timeout_secs / idle_timeout_secs / max_lifetime_secs。
- `PoolConfig::from_env()` — 读 `KOOIX_DB_MAX_CONNECTIONS` / `KOOIX_DB_MIN_CONNECTIONS` / `KOOIX_DB_ACQUIRE_TIMEOUT_SECS` / `KOOIX_DB_IDLE_TIMEOUT_SECS` / `KOOIX_DB_MAX_LIFETIME_SECS`，解析失败保留默认。
- `connect_with_config(url, &PoolConfig)` — 显式 API；旧 `connect(url, max)` 仍可用（自动 from_env + max 覆盖）。

### Changed

- 默认 max 16 → 20；新增 min=2（warm pool）；acquire_timeout 5s → 3s；新增 idle_timeout=600s + max_lifetime=1800s。这些值生产场景更合理：
  - **min=2 warm pool**：冷启后第一波突发流量不用排队等连接握手
  - **idle_timeout=600s**：多数云 LB（RDS / cloud SQL）5-15min 后回收空连接，提前 sqlx 内部关闭，避免下次 acquire 拿到死连接
  - **max_lifetime=1800s**：强制 30min 轮换，防长连接累积内存
- `crates/gate-server/src/main.rs`：postgres 启动改用 `PoolConfig::from_env()` + `connect_with_config`，并打 info log 显示生效参数。

### Why

product-review §1.5 判词：之前 pool 配置 silent / 不可调，只有 max（main.rs 硬编码 16）和 acquire_timeout（5s），缺 min / idle / lifetime —— 生产 LB 回收 + 突发流量 + 长连接累积三件套全踩坑。`from_env` 让运维不改代码即可调优。

### Verification

```bash
cargo check --workspace                          # 0 errors
cargo test -p gate-storage --lib pool_config     # 5 passed
```

新测试：
- `default_pool_config_is_safe` — 默认值合理性
- `env_override_max_connections` — 50 生效
- `env_min_connections_capped_by_max` — min 不超过 max
- `env_idle_timeout_zero_disables` — 0 → None（关闭 idle 回收）
- `env_bogus_values_fall_back_to_default` — parse 失败保留 default

---

## [0.4.70] — 2026-05-26

**主题**：Retry 加 ±25% jitter + stream-safe factory（product-review §2.4）。

### Added

- `RetryConfig.jitter: bool` — 默认 true。开启后 `backoff_ms(attempt)` 在 base ± 25% 范围内随机取值，防止 N 个客户端同步退避形成"雷暴"。
- `RetryConfig::stream_safe()` factory — `max_retries=0`。流式路径（chat_stream）一旦失败不能 retry（客户端已收 chunks + inflight pre-debit），用此 config 显式表达"非幂等"。

### Changed

- `backoff_ms` 实现改用 `saturating_pow` / `saturating_mul` 防 attempt 过大溢出。
- `rand::thread_rng().gen_range` 在 base ± span（base/4，最小 1ms）之间取 jitter。

### Why

product-review §2.4 判词：
- 原 retry 无 jitter，多客户端同时遇上游 502 后会在精确同一毫秒退避并重试 → 二次冲击放大问题；
- 流式 retry 没有明文禁止（chat.rs 现在恰好流式分支没调 with_retry，但 API 没语义表达"流式不能 retry"）；
- `backoff_ms` 用 `2u64.pow(attempt)` 在 attempt ≥ 64 时 panic。

### Verification

```bash
cargo check --workspace                       # 0 errors
cargo test -p gate-providers --lib            # 139 passed (134 + 5 新增 retry::tests)
cargo test -p gate-providers --lib retry::    # 5 passed
```

新测试：
- `stream_safe_disables_retry` — max_retries=0
- `backoff_without_jitter_is_deterministic_exponential` — 500/1000/2000/4000/cap10000
- `backoff_with_jitter_stays_within_25_percent_band` — 200 次采样落在 [1500, 2500]
- `backoff_ms_does_not_panic_on_huge_attempt` — attempt=64 不 panic
- `stream_safe_returns_immediately_on_first_error` — fn 只调一次

---

## [0.4.69] — 2026-05-26

**主题**：Provider error body 脱敏 — 截 512 字节 + SHA-256 哈希尾，防长 body 撑爆日志、防泄漏 PII（product-review A5）。

### Added

- `crates/gate-providers/src/error.rs::redact_upstream_body(&str) -> String` — body ≤ 512 字节原样保留；超过则截断 + 标注被截字节数 + SHA-256 前 16 字符哈希。UTF-8 边界感知。
- `ProviderError::upstream(status, body)` 工厂构造函数 — 所有上游 4xx/5xx 构造点统一走此入口，自动脱敏。
- `pub use error::redact_upstream_body` — server 层 audit 链可复用。

### Changed

- `crates/gate-providers/src/bedrock.rs`、`custom_provider/fastpath.rs` 的 `ProviderError::Upstream { body }` 改用 `ProviderError::upstream(status, body)` 工厂。

### Why

product-review §1.3 判词：`ProviderError::Upstream { body }` 是上游响应原文，上游 4xx 偶尔回显请求体（OpenAI tool_use error 已知）或敏感 header echo，进 audit/log/客户端响应可能泄漏 PII / key。

512 字节足够 debug 用，超长部分截掉但保留 hash 让排查时能定位原始 body；这是"防御性减少 blast radius"，不是替代 audit_redaction 的内容过滤。

### Verification

```bash
cargo check --workspace                       # 0 errors
cargo test -p gate-providers --lib            # 134 passed (130 + 4 新增 error::tests)
cargo test -p gate-providers --lib error::    # 4 passed
```

新测试：
- `redact_short_body_is_passthrough` — 短 body 原样
- `redact_long_body_truncates_with_hash` — 长 body 含 sha256 + 长度 < 原长
- `upstream_factory_redacts_long_body` — 工厂构造自动脱敏
- `redact_handles_utf8_boundary` — 多字节 UTF-8 跨越 512 不 panic

---

## [0.4.68] — 2026-05-26

**主题**：Usage 加 `cache_creation_input_tokens` + OpenAI o1/o3 reasoning_tokens 自动解析（product-review A4 真实剩余缺口）。

### Added

- `Usage.cache_creation_input_tokens: u32` — Anthropic prompt cache 写入 tokens（与 `cached_tokens` 即 cache_read 命中分别记账，定价不同）。
- `crates/gate-providers/src/openai.rs::lift_openai_usage_details` — 把 OpenAI 嵌套 `prompt_tokens_details.cached_tokens` 与 `completion_tokens_details.reasoning_tokens` 提到 `usage` 顶级。chat (非流) 与 sse_to_chunks (流) 路径都接入。

### Changed

- `crates/gate-providers/src/anthropic.rs`：
  - `AnthropicUsage` 新增 `cache_creation_input_tokens` 解析
  - `from_anthropic_response`（非流）回填 `Usage.cache_creation_input_tokens`
  - `StreamState` 加 `cache_creation_tokens` 字段；`MessageStart` 写入；`MessageDelta` 收尾回填到 final_usage
- `crates/gate-providers/src/openai.rs`：chat 路径改为 `bytes → Value → lift → ChatResponse`，比 `resp.json::<ChatResponse>` 多一道嵌套字段提升步骤
- `plugin_preset.rs` / `custom_provider/replay.rs`：Usage struct literal 补 `cache_creation_input_tokens: 0` / clone 字段

### Why

product-review §2.5 原判错（Anthropic 已经回填 `cached_tokens`）；正确剩余缺口：

1. Anthropic `cache_creation_input_tokens` 完全没解析 → billing 用 cache_creation 单独定价时少收
2. OpenAI o1/o3/o4-mini-reasoning 必返 nested details → gate 解析后丢失，下游 billing 拿不到 reasoning_tokens
3. Bedrock invocationMetrics 不在本版（留下版处理）

修后影响：Anthropic prompt caching 用户、OpenAI reasoning 模型用户，billing/usage rollup 拿到完整字段。

### Verification

```bash
cargo check --workspace                                  # 0 errors
cargo test -p gate-providers --lib                       # 130 passed (127 + 3 新增 lift_openai_usage_details)
```

新测试：
- `openai::openai_lift_tests::lift_cached_and_reasoning_tokens_from_details`
- `openai::openai_lift_tests::lift_no_details_is_noop`
- `openai::openai_lift_tests::lift_does_not_overwrite_explicit_top_level`

---

## [0.4.67] — 2026-05-26

**主题**：转译型 provider 透传 ChatRequest.extra — Anthropic / Bedrock 修补，OpenAI/Azure 已通过 `.json(&req)` 自动透传（product-review A3 修正）。

### Changed

- `crates/gate-providers/src/anthropic.rs` `AnthropicRequest` 加 `#[serde(flatten)] extra: Map<String, Value>`，`to_anthropic_request` 复制 `req.extra.clone()`。覆盖 `top_k` / `thinking` / `metadata` / `service_tier` 等 anthropic 特有字段。
- `crates/gate-providers/src/bedrock.rs` `ConverseRequest` 同样加 flatten extra，覆盖 `additionalModelRequestFields` / `guardrailConfig` / `toolConfig` / `promptVariables`。

### Why

OpenAI / Azure provider 用 `client.post().json(&req)` 直接序列化 `ChatRequest`，由于 `ChatRequest.extra` 已经是 `#[serde(flatten)]` 字段，这两条路径其实早就透传了。原审查报告 §2.2 判错。

但 Anthropic / Bedrock 走的是"转译"路径（`to_anthropic_request` / `to_converse_request`），把 ChatRequest 字段一一映射成 provider 私有结构体后再序列化——`extra` 在转译过程中被丢弃，前端用户传的 `top_k`、`thinking`、`additionalModelRequestFields` 等字段会"看上去能传但实际丢"。这是真缺口。

### Verification

```bash
cargo check --workspace                                     # 0 errors
cargo test -p gate-providers --lib                          # 127 passed (124 既有 + 3 新增)
cargo test -p gate-providers --lib extra_fields             # 2 passed (anthropic + bedrock)
cargo test -p gate-providers --lib empty_extra              # 1 passed
```

新增测试：
- `anthropic::tests::extra_fields_passthrough_into_anthropic_body` — 验 `top_k` / `thinking` / `metadata` 透传到顶级 JSON
- `anthropic::tests::empty_extra_does_not_emit_keys` — 验空 extra 不污染输出
- `bedrock::tests::extra_fields_passthrough_into_converse_body` — 验 `additionalModelRequestFields` / `guardrailConfig` 透传

---

## [0.4.66] — 2026-05-26

**主题**：gate_chat_* 维度 metrics — chat handler 加 e2e latency / TTFB / SSE chunk count（product-review A2）。

### Added

- `gate_chat_requests_total{model, provider_type, streaming, outcome}` — chat handler 请求计数
- `gate_chat_duration_seconds{model, provider_type, streaming, outcome}` — chat handler e2e 耗时 histogram
- `gate_chat_ttfb_seconds{model, provider_type}` — 流式首 chunk 延迟 histogram
- `gate_chat_stream_chunks_total{model, provider_type, outcome}` — 单个 stream 累计 chunk 数

### Changed

- `crates/gate-server/src/metrics.rs` 新增 3 个 emit 函数：`record_chat_request` / `record_chat_ttfb` / `record_chat_stream_chunks`，标签经 `normalize_label_value` 卡死基数。
- `crates/gate-server/src/routes/chat.rs` 在 4 个收口点埋指标：
  - 流式上游建立失败 → `chat_request(streaming=true, error)`
  - 流式 inspect 首 chunk → `chat_ttfb`
  - 流式 trigger 收尾 → `chat_request(streaming=true, ok|error)` + `chat_stream_chunks`
  - 非流式 Ok / Err 各一次 → `chat_request(streaming=false, ok|error)`
- `install_recorder` 为新 histogram 设置 `REQUEST_DURATION_BUCKETS`。

### Why

product-review §1.2 判词：metrics 套件设计齐但**chat 维度盲区**——已有 `gateway_stage_duration_seconds` / `gateway_requests_total` 是按 HTTP method/path/stage 维度，无法按 model+provider 切片 LLM 体验。缺 TTFB → 用户感知首包延迟没法 SLO；缺 chunk count → 流式吞吐无法监控；缺 outcome 维度的 chat latency → 错误请求和成功请求 p99 混在一起。

### Verification

```bash
cargo check --workspace                              # 0 errors
cargo test -p gate-server --lib                      # 45 passed (44 既有 + 1 新增 chat_metrics_emit)
cargo test -p gate-server --lib metrics::            # 8 passed
```

新增测试 `chat_metrics_emit_through_recorder`：调 4 个 record_chat_* fn 后 render prometheus，断言输出包含全部 4 个 metric name + 关键标签（streaming/outcome/provider_type）。

---

## [0.4.65] — 2026-05-26

**主题**：SharedHttpClient — 4 个 fast-path provider 共享 reqwest::Client，避免每 channel 一个独立连接池（product-review A1）。

### Changed

- `crates/gate-providers/src/lib.rs` 新增 `shared_http_client(&ProviderOpts)`：按 (connect_timeout, total_timeout) 维度缓存 `Arc<reqwest::Client>`，LRU 上限 8。
- `OpenAiProvider` / `AnthropicProvider` / `AzureProvider` / `BedrockProvider` 改持 `Arc<reqwest::Client>`，构造时调 `shared_http_client(&opts)` 复用全局池。
- `CustomHttpProvider` 不变：仍走独立 builder（依赖 sandbox DNS resolver + redirect=none + manifest 自带 timeout override）。
- 暴露测试辅助 `_reset_shared_http_clients()`。

### Why

每个 channel 一份独立 reqwest pool 在多 channel 共享同上游 base_url 场景下浪费 TCP/TLS 握手与 HTTP2 multiplexing。SharedHttpClient 让相同 timeout bucket 内 N channel 走同一 connection pool，高并发下连接数从 O(N×C) 降到 O(C)（C ≤ 8）。

### Verification

```bash
cargo check --workspace                                    # 0 errors
cargo test -p gate-providers --lib                         # 124 passed (122 既有 + 2 新增 SharedHttpClient)
cargo test -p gate-providers --lib shared_client_tests     # 2 passed
```

新增测试：
- `shared_clients_with_same_opts_are_identical_arc` — 验证 same opts → Arc::ptr_eq
- `shared_clients_with_different_opts_are_distinct` — 验证不同 timeout 桶不混用

---

## [0.4.64] — 2026-05-23

**主题**：admin/groups 抽 BindingTable — 渠道列表 + inline editing 整体抽出。

### Changed

- `web/src/routes/admin/groups/+page.svelte` 739 → 655（-84 行 / -11.4%）
- 新增 `_components/BindingTable.svelte`（180 行）：渠道列表 DataTable + inline editing + Project references；用 17 props（detail/refs/4 editing state/bindingCapabilities + 9 callback）
- 父保留 inline editing state（editingBindingId / editBindingPriority / editBindingWeight / editBindingCanaryPercent），子通过 onUpdate* 回调更新

### Verification

```bash
npm run check    # 0/0
npm test         # 13/87
```

---

## [0.4.63] — 2026-05-23

**主题**：admin/groups 抽 CanaryComparePanel — Canary 对比面板独立组件 + 6 个 canary helper 移至 _lib。

### Changed

- `web/src/routes/admin/groups/+page.svelte` 845 → 739（-106 行 / -12.5%）
- 新增 `_components/CanaryComparePanel.svelte`（87 行）：Canary stats DataTable + delta 渲染
- `_lib/helpers.ts` 扩 6 个：`metricDelta` / `weightedBaseline` / `formatMaybeMs` / `formatMaybeMicros` / `formatSignedPercentDelta` / `formatSignedNumberDelta`

### Verification

```bash
npm run check    # 0/0
npm test         # 13/87
```

---

## [0.4.62] — 2026-05-23

**主题**：admin/groups 抽 FallbackChainPanel — 回退链路面板独立组件。

### Changed

- `web/src/routes/admin/groups/+page.svelte` 923 → 845（-78 行 / -8.5%）
- 新增 `_components/FallbackChainPanel.svelte`（90 行）：fallback chain stats grid + 链路 visualizer 整体抽出
- `_lib/helpers.ts` 扩 2 个：`formatPercent` / `formatCanaryPercent`

### Verification

```bash
npm run check    # 0/0
npm test         # 13/87
```

---

## [0.4.61] — 2026-05-23

**主题**：admin/groups 抽 GroupCard + _lib/helpers — 0.4.61-0.4.90 前端打磨阶段 A 启航。

### Changed

- `web/src/routes/admin/groups/+page.svelte` 972 → 923（-49 行 / -5.0%）
- 新增 `_components/GroupCard.svelte`（76 行）：grid item 整体抽出，props=group/isSelected/groupName/onSelect/onToggleEnabled
- 新增 `_lib/helpers.ts`：`STRATEGIES` / `PROVIDER_COLOR` / `strategyMeta` / `strategyBadgeClass` / `capabilityChipClass` / `formatNumber` 6 个 helper 抽出，page 与 GroupCard 共用

### Verification

```bash
npm run check    # 0 errors / 0 warnings
npm test         # 13 / 87 passed
cargo check --workspace  # ok
```

---

## [Unreleased] (legacy — 第一/二/三轮文档收口已 push 到 main，未发版)

### Docs — 第一轮：v0.4.60 → v0.5.0 product-gaps 与 ADR-0003 实装收口

- **新增** [docs/product-gaps.md](./docs/product-gaps.md) — v0.4.60 → v0.5.0 产品化缺口对账清单（17 项 G-编号，按 P0/P1/P2/P3 分组，含影响面 / 当前状态 / 实施路径 / 验收门禁 / 关联引用）。0.5.0 启动会议据此筛选。
- **修订** [ADR-0003](./docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md) Status: PoC accepted → **Implemented (0.4.16 PoC → 0.4.60 完整产品形态)**；Verification 章节按 0.4.16 / 0.4.21-0.4.60 / v0.5.0 候选三段重写，对账实际命中位置；Negative/Risks 与 References 同步更新。
- **修订** [docs/wasm-plugin-abi.md](./docs/wasm-plugin-abi.md) Status: PoC v0 → **v0 完整产品形态**；末尾新增 0.4.x 实装对账表 + v0.5.0 未命中项指向 product-gaps；非目标节标注 wasmtime runtime 已落地。
- **修订** [docs/wasm-sdk-as.md](./docs/wasm-sdk-as.md) Status: 文档先行 → **v0 package 已落地（0.4.55-0.4.56）**；进度章节标完成项；参考链接新增 sdks/examples/G-101 入口。
- **修订** [docs/README.md](./docs/README.md) — 关键文档表收录 6 篇 0.4.x 新文档（getting-started / observability / wasm-runbook / wasm-sdk-as / manifest-registry-signature / api-reference / playground）+ product-gaps；阅读顺序补 getting-started 与 product-gaps。
- **修订** [docs/getting-started.md](./docs/getting-started.md) Helm `image.tag` v0.4.28 → v0.4.60。
- **修订** [ROADMAP.md § M4](./ROADMAP.md#m4--v050--enterprise--saas-进阶候选) v0.5.0 候选改为 P0/P1/P2 三档分组（17 项 G-编号），并链入 product-gaps.md。

> 第一轮已 commit `a5eabc1`（本地未 push）。

### Docs — 第二轮文档收口（漂移修复 + crate README 全覆盖）

- **修订** README badge 与测试段：`tests-485+ Rust + 87 web` 与正文 263 行 `285 Rust` 互相矛盾，改为统一 `485 Rust + 86 web tests`（实测：lib 242 + 全量 485；web 86 pass / 1 fail 漂移留待单独修）。
- **修订** [README.md](./README.md) 文档地图：从 8 行扩为分组（入门/部署、架构/设计、扩展面、API/接入、可观测/运维、路线/缺口），收录 product-gaps / wasm-plugin-abi / wasm-sdk-as / manifest-registry-signature / getting-started / observability / api-reference / wasm-runbook / threat-model 共 9 篇。
- **修订** [README.md](./README.md) Workspace 结构：补 `crates/gate-wasm/` `crates/gate-wasm-sdk/` `sdks/` `deploy/` `bench/` 五项；`gate-providers` 注释加 WASM 集成。
- **修订** [DESIGN.md](./DESIGN.md) 四处过期表述：原则 #5 / § 4 plugin 整流尾段 / § 关键决策表第 7 行 / § 核心交付清单（WASM ABI 设计稿 → v0 完整实装；master key 轮换工具改回 [ ] 与现状一致）。
- **修订** [ROADMAP.md](./ROADMAP.md) 测试基线：`285 Rust test list entries` → `485 Rust（lib 242 + integration/doctest 243）+ 86 web tests`。
- **新增** 9 个 crate README：`gate-core` / `gate-storage` / `gate-crypto` / `gate-auth` / `gate-cache` / `gate-billing` / `gate-server` / `gate-wasm` / `gate-wasm-sdk`。两个新 crate（`gate-wasm` `gate-wasm-sdk`）写完整模块表 + 资源限制 + 失败语义；7 个老 crate 写最小自介，避免无效膨胀。
- **修订** [crates/gate-providers/README.md](./crates/gate-providers/README.md) 演进表：补 0.4.21-0.4.60 WASM transform 集成行；参考链接补 ADR-0002 / ADR-0003 / gate-wasm / gate-wasm-sdk。
- **修订** [docs/plugin-manifest.md](./docs/plugin-manifest.md) 后续计划段：删除「WASM runtime PoC」（已实装）+ 链入 ADR-0003 + product-gaps G-103。
- **修订** [docs/architecture.md](./docs/architecture.md) 设计选择表第 5 行：`compile-time provider + HTTP Plugin manifest` → 加 WASM transform v0；触发条件改 ABI v1 / wit-bindgen。

### Notes

- web 测试发现 1 处漂移：`web/src/tests/ui-copy.test.ts` 仍期望 `pricing/+page.svelte` 包含字面量 `'Pricing wizard 向导'`，但已抽到 `_components/PricingWizard.svelte`。属前次组件化拆分遗留，**不在本轮 docs scope**，留作后续单独修。

### Verification

```bash
KOOIX_SKIP_PG_TESTS=1 cargo test --workspace --no-fail-fast | grep 'test result' | awk '{s+=$4}END{print s}'  # 485
git diff --stat                                                                                                # 6 modified
git ls-files --others --exclude-standard | wc -l                                                               # 9 new READMEs
```

---

## [0.4.60] — 2026-05-23

**主题**：0.4.x 60 版本完整产品阶段宣告 — M3 全部产品化交付，0.5.0 真正进入下一阶段。

### 0.4.51-0.4.60 阶段战报

| 版本 | 主战 | 0.5.0 候选清单 |
|------|------|---------------|
| 0.4.51 | SSE pipeline stream_chunk_transform 真接通 | ✓ 第 1 项清零 |
| 0.4.52 | SSE e2e 测试 (wiremock + 真模块) | — |
| 0.4.53 | manifest registry signature schema 加 typed key_id/alg + 文档 | — |
| 0.4.54 | minisign 格式校验 + cosign base64 校验 | ✓ 第 2 项部分清零（schema/format ✓ / 真公钥验签留 0.5.0） |
| 0.4.55 | sdks/gate-wasm-sdk-as npm package | ✓ 第 3 项清零（本地包 ✓ / npm publish 留 0.5.0） |
| 0.4.56 | examples/wasm-transform-as 实战示例 | — |
| 0.4.57 | ProviderRouter wasm_host setter/getter | — |
| 0.4.58 | gate_plugin_wasm_calls_total Prometheus describe | — |
| 0.4.59 | ROADMAP M3 完整产品形态 + M4 候选 | — |
| 0.4.60 | 完整产品宣告 | — |

### 完整产品形态最终验收

#### M3 ADR-0003 v0 全栈

```
[wasm 模块作者]
   │  Rust SDK (gate-wasm-sdk)              ✓ 0.4.27
   │  AssemblyScript SDK (sdks/gate-wasm-sdk-as)  ✓ 0.4.55
   │  examples/wasm-transform (Rust)         ✓ 0.4.33
   │  examples/wasm-transform-as (AS)        ✓ 0.4.56
   ↓
[模块二进制]
   │  kgctl wasm verify/inspect              ✓ 0.4.45
   │  manifest registry signature schema     ✓ 0.4.53
   │  cosign / minisign / sigstore_bundle  format ✓ 0.4.54
   ↓
[manifest 注册]
   │  channel.security.wasm typed schema     ✓ 0.4.23
   │  WASM_MANIFEST_SAMPLE admin UI 提示     ✓ 0.4.48
   │  ProviderRouter wasm_host setter/getter ✓ 0.4.57
   ↓
[runtime]
   │  gate-wasm crate (wasmtime 26)          ✓ 0.4.21
   │  WasmtimeHost + sha256 + fuel + memcap  ✓ 0.4.22
   │  3 hook 全接通 (chat_request/response/stream_chunk) ✓ 0.4.24/25/51
   │  fallback policy + Prometheus metric    ✓ 0.4.26
   │  CustomHttpProvider 集成 (chat / chat_stream) ✓ 0.4.42-0.4.44/51
   │  e2e 测试 (chat + stream)               ✓ 0.4.46/52
   │  /metrics describe HELP                 ✓ 0.4.58
   ↓
[运营]
   │  docs/wasm-runbook.md 故障手册          ✓ 0.4.35
   │  docs/threat-model.md WASM 表面分析     ✓ 0.4.36
   │  docs/manifest-registry-signature.md    ✓ 0.4.53
   │  Criterion bench wasm_invoke            ✓ 0.4.37
```

#### 工程总账（0.4.60 结算）

```
Tags                    60 个 (v0.4.0 → v0.4.60)
Rust 后端                71047+ 行 (含 gate-wasm + gate-wasm-sdk 2 个新 crate)
SDK npm package          1 个 (sdks/gate-wasm-sdk-as)
前端 web                 21720 行
前端 _components         22+ 个
前端 _lib helper         5+ 个
ADR                      3 个 (ADR-0001 / 0002 / 0003 全部 ✅)
Helm chart               1 套 (deploy/helm/gate)
Grafana dashboard        1 个
WASM 端到端 e2e 测试     5 个 (3 unit + 2 stream + integration)
Bench                    4 个 (plugin_vs_builtin + sse + routing + wasm_invoke)
clippy                   workspace 0/0
```

### 0.5.0 真候选（不再标 deferred，等启动会议筛选）

详见 [ROADMAP.md M4 章节](./ROADMAP.md#m4--v050--enterprise--saas-进阶候选)。

### 完整产品宣告

> 0.4.60 起 Kooix Gate 进入 **完整产品形态**：
> - LLM 网关核心：18+ provider preset / 多 Org RLS / 流式 fail-closed 计费 / typed ID
> - WASM Plugin v0：双 SDK + 4 hook（含 SSE）+ fallback + 验签 schema + 完整运维
> - 部署：Helm chart + 三档 quickstart + 5 个 runbook + 威胁模型
> - DX：OpenAPI / Postman / Bruno / examples × 11 / kgctl 全套

可正式 release 0.5.0 启动会议。

### Verification

- `cargo clippy --workspace --all-targets -- -D warnings` 0/0
- `cargo test --workspace --lib` 全过
- `cargo test -p gate-providers --test wasm_integration` 4 passed (含 SSE stream e2e)
- `cargo build` 净

---

## [0.4.59] — 2026-05-23

**主题**：ROADMAP 同步 — M3 完整产品形态宣告 + M4 v0.5.0 候选方向章节。

### Changed

- `ROADMAP.md` M3 WASM 行：从 "PoC 收口 + runtime 留 0.5.0+" 改为 "**完整产品形态**"（0.4.16 → 0.4.58 全程脉络）
- `ROADMAP.md` 新增 **M4 v0.5.0 候选方向** 章节：
  - 真公钥验签链（sigstore-rs 接入）
  - SaaS 多区域路由
  - SCIM v2
  - WASM auto-mount runtime（builder 集成）
  - AssemblyScript SDK npm publish
  - Web bundle 220 → 180KB
  - 管理面 wasm form UI

---

## [0.4.58] — 2026-05-23

**主题**：gate_plugin_wasm_calls_total 注册到 Prometheus exporter — /metrics 显示 HELP 行。

### Added

- `gate-server/src/metrics.rs install_recorder`：`describe_counter!("gate_plugin_wasm_calls_total", ...)` 注入 ADR-0003 HELP 文案
- /metrics 端点 scrape 时 wasm metric 自带说明

### Verification

- `cargo build -p gate-server` 净

---

## [0.4.57] — 2026-05-23

**主题**：ProviderRouter 持有 wasm_host — auto-mount 通路就绪。

### Added

- `ProviderRouter`:
  - `wasm_host: Option<Arc<dyn gate_wasm::WasmHost>>` 字段
  - `with_wasm_host(host)` builder
  - `wasm_host()` getter
- `build_provider` 注释说明 0.5.0 完整 auto-mount 路径

### Verification

- `cargo build -p gate-providers` 净

### Note

完整 auto-mount（builder 自动调 host.load_module + with_wasm_host）需要 channel manifest 解析后从 wasm.module 字段拿 module bytes 并加载——涉及外部存储读写，0.5.0 接 ChannelKeyRepo 模式落地。当前 setter/getter 就绪，调用方可手工组合。

---

## [0.4.56] — 2026-05-23

**主题**：examples/wasm-transform-as AssemblyScript 实战示例。

### Added

- `examples/wasm-transform-as/`：完整可编译 AssemblyScript transform 示例
  - `package.json` + `asconfig.json`
  - `assembly/index.ts`：完整 gate_alloc / chat_request / chat_response identity transform
  - `README.md`：编译/部署 + 与 Rust SDK 示例对比表

### Fixed

- `examples/README.md`：补回 0.4.33 漏的 `wasm-transform/` 索引行 + 加 `wasm-transform-as/`

---

## [0.4.55] — 2026-05-23

**主题**：sdks/gate-wasm-sdk-as npm package — AssemblyScript SDK 完整落地。

### Added

- `sdks/gate-wasm-sdk-as/`
  - `package.json` — @kooix-gate/wasm-sdk-as 0.4.55，assemblyscript devDep
  - `assembly/index.ts` — 完整 ABI v0 helpers
    - `gate_alloc(size)` export
    - `encodeReturn(ptr, len)` 工具
    - `returnPayload(buf)` 写 linear memory
    - `withInput(ptr, len, fn)` 完整封装
  - `asconfig.json` — debug + release target
  - `README.md` — Quickstart + API 表 + 参考

---

## [0.4.54] — 2026-05-23

**主题**：minisign signature 格式校验 + cosign/sigstore base64 校验 — registry 验签实质化。

### Added

- `verify_minisign_format`：base64 解码 + 长度 ≥ 64B 检查 + key_id base64 校验
- cosign / sigstore_bundle：base64 strict decode 校验
- `validate_signature` 升级为 per-kind dispatch
- deps：`ed25519-dalek = "2"` (留 0.5.0+ 真公钥验签用) + `base64.workspace`

### Verification

- `cargo clippy --workspace --all-targets -- -D warnings` 0/0

---

## [0.4.53] — 2026-05-23

**主题**：manifest registry signature schema 加 typed key_id/alg 字段 + 文档化。

### Added

- `RegistrySignature.key_id` / `alg` 可选字段（serde skip_serializing_if=None）
- `docs/manifest-registry-signature.md`：完整 cosign / minisign / sigstore_bundle / unsigned 4 种签名模式文档
  - 工具命令示例
  - registry.json 字段示例
  - 当前实现进度（schema typed ✅ / 真实验签 0.4.54）
  - Trust chain 流程图

### Verification

- `cargo build -p kgctl` 净

---

## [0.4.52] — 2026-05-23

**主题**：stream_chunk_transform e2e — wiremock SSE + 真 wasm 模块完整验证。

### Added

- `custom_provider_with_wasm_host_streams_chunks` 测试
  - mock SSE 端点：2 个 chunk + [DONE]
  - 真 wasm 模块 export chat_request_transform + stream_chunk_transform
  - chat_stream() 完整链路，concat content 与原 SSE payload 一致

### Verification

- `cargo test -p gate-providers --test wasm_integration` 4 passed (+1 stream)

---

## [0.4.51] — 2026-05-23

**主题**：chat_stream SSE pipeline 真接通 stream_chunk_transform — 0.5.0 候选清第 1 项。

### Changed

- `Provider::chat_stream`：在 `resp.bytes_stream()` 与 `normalize_plugin_sse` 之间插 `futures::stream::then` 包装，每 chunk 走 `gate_wasm::invoke_with_fallback`
- 仅在 `manifest.security.wasm.is_some()` 且 `wasm_host` 注入时启用；否则零开销 passthrough
- `wasm_transform_stream_chunk` helper 保留作公共 API（仍 #[allow(dead_code)]，inline 已直接调用 invoke_with_fallback）

### Verification

- `cargo build -p gate-providers` 净
- `cargo clippy --workspace --all-targets -- -D warnings` 0/0

---

## [0.4.50] — 2026-05-23

**主题**：0.4.41-0.4.50 中阶段收尾 — WASM 集成全链路 e2e 走通，gate-providers 完整产品形态。

### 0.4.41-0.4.50 阶段战报

| 版本 | 主战 |
|------|------|
| 0.4.41 | CustomHttpProvider mount WasmHost (struct + with_wasm_host builder) |
| 0.4.42 | chat_request hook 接通 (chat() build body 后 wasm transform) |
| 0.4.43 | chat_response hook 接通 (limited_json_response_with_wasm) |
| 0.4.44 | chat_stream request hook (stream chunk 留 0.5.0+) |
| 0.4.45 | kgctl wasm verify/inspect 子命令 |
| 0.4.46 | wasm e2e 测试（wiremock + 真 wasm 模块 + 完整 chat 链路） |
| 0.4.47 | clippy 0/0 全清 |
| 0.4.48 | admin UI WASM_MANIFEST_SAMPLE 模板 |
| 0.4.49 | docs/wasm-sdk-as.md AssemblyScript SDK 文档 |
| 0.4.50 | 中阶段收尾 |

### WASM 集成完整能力（0.4.50 结算）

```
client → gate-server → CustomHttpProvider
                          │
                          ├─ wasm_transform_request   (0.4.42 ✓)
                          ↓
                       reqwest::post(upstream)
                          ↓
                          ├─ wasm_transform_response  (0.4.43 ✓)
                          ↓
                       parse JSON → return ChatResponse

                       wasm_transform_stream_chunk    (helper ready, SSE pipeline 留 0.5.0+)
```

### 工程总账（0.4.50 结算）

- Rust 后端：71047+ 行（gate-wasm + gate-wasm-sdk）
- Tags：50 个 (v0.4.0 → v0.4.50)
- 新增 crates：2 个（gate-wasm / gate-wasm-sdk）
- 新增 e2e 测试：3 个（wasm_integration.rs）
- workspace tests：485+ 通过
- WASM 完整 runtime + Rust SDK + AssemblyScript 文档 + Helm chart + Grafana dashboard + threat model + runbook 全套就绪

### 仅剩 0.5.0 候选

- SSE pipeline 内 stream_chunk_transform 接通
- Manifest registry + Sigstore 签名链
- AssemblyScript SDK npm package
- SaaS 多区域 / SCIM v2

### Verification

- `cargo clippy --workspace --all-targets -- -D warnings` 0/0
- `cargo test --workspace --lib` 全过
- 50 个 tag 全部 push origin/main

---

## [0.4.49] — 2026-05-23

**主题**：docs/wasm-sdk-as.md — AssemblyScript SDK 文档先行。

### Added

- `docs/wasm-sdk-as.md`：完整 AssemblyScript SDK 文档
  - 初始化 + 实现 `chat_request_transform` / `chat_response_transform`
  - ABI v0 helpers 在 AS 中的最小实现（gate_alloc / readInput / returnPayload）
  - asconfig.json + 编译命令
  - 限制对比表（AS vs Rust SDK）
  - 0.5.0+ npm package 计划

---

## [0.4.48] — 2026-05-23

**主题**：admin UI wasm sample 模板 — 用户复制即可配 wasm 字段。

### Added

- `channels/_lib/helpers.ts` 新增 `WASM_MANIFEST_SAMPLE` const：完整 wasm 字段 manifest sample（preset + security.wasm + hooks）
- 用户在 Manifest 文本框可粘贴 sample，替换 sha256 / module 路径即可

### Why

- ADR-0003 v0 wasm 字段已在 0.4.23 typed schema 落地，admin UI 通过 manifest 文本框已能配
- 本版补 sample 减少 onboarding 摩擦，完整 wasm form UI 留 0.5.0+

### Verification

- `npm run check` 0/0

---

## [0.4.47] — 2026-05-23

**主题**：lint 全清 — `cargo clippy --workspace --all-targets -- -D warnings` 0/0。

### Fixed

- `crates/gate-wasm/src/wasmtime_host.rs` `ChannelModule.sha256` 加 `#[allow(dead_code)]` + 注释（保留作 audit/observability）

### Verification

- `cargo clippy --workspace --all-targets -- -D warnings` 净
- 工程 0 warning 0 error 状态

---

## [0.4.46] — 2026-05-23

**主题**：WASM e2e 集成测试 — wiremock + 真 wasm 模块 + CustomHttpProvider 完整链路。

### Added

- `crates/gate-providers/tests/wasm_integration.rs`（3 测试，全过）：
  - `custom_provider_with_wasm_host_round_trips_chat` — 真 wasm 模块 + manifest.security.wasm + with_wasm_host()，完整 chat() 往返
  - `custom_provider_without_wasm_skips_transform` — 未配置 wasm 字段时跳过路径
  - `invoke_with_fallback_wraps_real_module` — fallback wrapper + real 模块联动
- gate-providers dev-dep：`wat = "1"` + `gate-wasm = { path = "../gate-wasm" }`

### Verification

- `cargo test -p gate-providers --test wasm_integration` 3 passed

### M3 → M4 里程碑

WASM 集成已 e2e 走通 — 0.4.40 阶段总结里 "gate-providers WASM 集成" 不再 deferred 到 0.5.0。

---

## [0.4.45] — 2026-05-23

**主题**：`kgctl wasm verify / inspect` 子命令 — wasm 模块工具链。

### Added

- `kgctl wasm verify <path>`：sha256 + 文件大小，输出可粘贴的 manifest 片段
- `kgctl wasm inspect <path>`：检查 ABI v0 必要 export（memory / gate_alloc）+ 列出 hooks
- `crates/kgctl/src/wasm.rs`：实现
- 依赖 `wasmparser 0.218`

### Verification

- `cargo build -p kgctl` 净

---

## [0.4.44] — 2026-05-23

**主题**：Provider::chat_stream 接通 wasm chat_request_transform — stream chunk hook 留 0.5.0+。

### Changed

- `Provider::chat_stream`：build body 后插 `wasm_transform_request(body, &req)`
- `wasm_transform_stream_chunk` 标 `#[allow(dead_code)]` — SSE pipeline 接通留 0.5.0+
  - 原因：SSE normalizer 在 host 端做归一，wasm transform 与 sse_to_chunks 顺序需仔细设计
  - 当前 helper 已就绪，只缺调用点

### Verification

- `cargo build -p gate-providers` 净 (0 warning)

---

## [0.4.43] — 2026-05-23

**主题**：Provider::chat 接通 wasm chat_response_transform — 集成 step 3。

### Added

- `limited_json_response_with_wasm(resp, model)`：读 raw body → wasm transform → parse JSON

### Changed

- `Provider::chat`：用 `limited_json_response_with_wasm(resp, &req.model)` 替代普通版

### Verification

- `cargo build -p gate-providers` 净（仅 stream_chunk dead_code，0.4.44 接通后清）

---

## [0.4.42] — 2026-05-23

**主题**：CustomHttpProvider chat() 接通 wasm chat_request_transform — 集成 step 2。

### Added

- 3 个 wasm transform helper（一次性加完，0.4.43/0.4.44 接 response/stream 调用即可）：
  - `wasm_transform_request(body, req)` — 已在 0.4.42 chat() 接通
  - `wasm_transform_response(body, model)` — 0.4.43 接通
  - `wasm_transform_stream_chunk(chunk, model)` — 0.4.44 接通
- 全部走 `gate_wasm::invoke_with_fallback`：失败永不 propagate，identity passthrough

### Changed

- `Provider::chat`：build body 后插入 `wasm_transform_request(body, &req)`

### Verification

- `cargo build -p gate-providers` 净（仅 dead_code warning，0.4.43/0.4.44 接调用后清）

---

## [0.4.41] — 2026-05-23

**主题**：CustomHttpProvider mount 可选 WasmHost — wasm 集成第一步。

### Added

- `gate-providers/Cargo.toml`：依赖 `gate-wasm = { path = "../gate-wasm" }`
- `CustomHttpProvider` struct 加两字段：
  - `wasm_host: Option<Arc<dyn gate_wasm::WasmHost>>`
  - `wasm_channel_id: String`
- `with_wasm_host(host, channel_id)` builder：router 创建 provider 后注入 wasm host

### Verification

- `cargo build -p gate-providers` 净

---

## [0.4.40] — 2026-05-23

**主题**：0.4.x 41 版本大终结篇 — M3 全结、WASM v0 落地、产品化打磨完毕。准备 0.5.0 启动。

### 0.4.x 全程战报（41 版本 0.4.0 → 0.4.40）

| 阶段 | 版本范围 | 主战 |
|------|---------|------|
| **里程碑收口** | 0.4.0 | M3 Fast-path Runtime (ADR-0002) |
| **Rust 拆解** | 0.4.1 | 三巨兽全部 -52%+ |
| **前端组件化 P1** | 0.4.2-0.4.10 | channels 1864 → 1487 (-20.2%) |
| **前端组件化 P2** | 0.4.11-0.4.15 | ChannelTable / QuotaWizard / PricingWizard / SessionModal |
| **WASM PoC** | 0.4.16 | ADR-0003 v0 设计稿冻结 |
| **前端清债** | 0.4.17-0.4.19 | channelId modals / bundle 220KB / billing helpers |
| **0.4.x 中盘大收尾** | 0.4.20 | M3 全结 ticked |
| **WASM Runtime 落地** | 0.4.21-0.4.27 | gate-wasm crate + 3 hook + fallback + SDK |
| **产品化打磨** | 0.4.28-0.4.39 | README / getting-started / kgctl / Helm / OpenAPI / examples / observability / runbook / threat-model / bench / architecture / RELEASE |
| **大终结** | 0.4.40 | 准备 0.5.0 |

### 0.4.x WASM Plugin 完整能力（ADR-0003 v0）

- ✅ `crates/gate-wasm` 基于 wasmtime 26
- ✅ 3 hook 全接通：chat_request / chat_response / stream_chunk
- ✅ SHA256 强校验 + fuel/memory hard limit + sandbox
- ✅ fallback policy：panic / OOM / timeout 全降级 identity，业务不挂
- ✅ `crates/gate-wasm-sdk` 用户写 Rust transform
- ✅ `examples/wasm-transform/` 实战示例
- ✅ Prometheus metric `gate_plugin_wasm_calls_total{channel,hook,status}`
- ✅ `docs/wasm-runbook.md` 故障手册
- ✅ `docs/threat-model.md` 威胁建模含 WASM 表面
- ✅ Criterion bench `wasm_invoke`

### 工程总账（0.4.40 结算）

```
Rust 后端       71047 行 (含 gate-wasm 新 crate + gate-wasm-sdk)
前端 web       21720 行 (svelte + ts)
工程全量      103400 行 (含 docs / examples / migrations)
Tags           41 个 (v0.4.0 → v0.4.40)
新增 crate     2 个 (gate-wasm + gate-wasm-sdk)
新增 _components 22+ 个
新增 _lib 模块 5+ 个
新增 ADR        1 个 (ADR-0003 WASM Plugin ABI v0)
新增 dashboard 1 个 (kooix-gate-overview)
新增 Helm chart 1 套 (deploy/helm/gate)
```

### M3 完结声明

- ADR-0001 Providers as Plugin ✅
- ADR-0002 Fast-path Runtime ✅
- **ADR-0003 WASM Plugin ABI v0 ✅** (0.4.16 设计 + 0.4.21-0.4.27 实现)

### 0.5.0 启动书

下一阶段候选主战：

| 候选 | 描述 | 估时 |
|------|------|------|
| **gate-providers WASM 集成** | 把 gate-wasm 集成到 `CustomHttpProvider` chat / response / stream 路径 | 2 周 |
| **manifest registry + 签名** | cosign / Sigstore 模块信任链 | 2 周 |
| **AssemblyScript SDK** | gate-wasm-sdk-as：用 TypeScript 写 transform | 1 周 |
| **SaaS 多区域路由** | 跨 region failover / data sovereignty | 3 周 |
| **企业 SCIM 完整化** | user / group / role sync v2 | 1 周 |

详见即将开 `ROADMAP.md` 的 M4 / M5 章节。

### Verification

- `cargo build` 净
- `cargo test --workspace --lib` 全过（含 gate-wasm 13 tests / gate-wasm-sdk 1 doctest ignored / 既有 485 测试）
- 41 个 tag 全部 push origin/main
- CHANGELOG / ROADMAP / README / docs/architecture / docs/getting-started 全部同步

---

## [0.4.39] — 2026-05-23

**主题**：RELEASE.md 0.4.x 阶段补充 — 标准 commit pipeline + GHA fallback + WASM 模块发布。

### Added

- `RELEASE.md` 末尾追加 "0.4.x 阶段补充（2026-05-23）" 章节
  - 标准 commit pipeline（bump / fmt-clippy-test / CHANGELOG / commit / tag / push）
  - Docker / Release artifact 本地手工补（GHA billing fallback）
  - WASM 模块发布流程（编译 + sha256 + gh release upload）
  - 阶段大版每 10 patch 一次的固定动作（ROADMAP / CHANGELOG / README / getting-started）

---

## [0.4.38] — 2026-05-23

**主题**：架构文档收口 — worker-plane 加 ADR-0003 WASM Plugin 章节。

### Changed

- `docs/architecture.md`：last_verified 2026-05-21 → 2026-05-23
- `docs/architecture/worker-plane.md`：
  - last_verified 更新
  - 代码锚点新增 `crates/gate-wasm/` 完整说明
  - 新增 "ADR-0003 WASM Plugin Worker（0.4.x）" 章节，含 chat 入站全链路图

---

## [0.4.37] — 2026-05-23

**主题**：gate-wasm bench — wasm_invoke_hook 单次调用开销测量。

### Added

- `crates/gate-wasm/benches/wasm_invoke.rs`：Criterion bench
  - `wasm_invoke_hook/memory_copy/{128,1024,10240}` — payload scaling
  - `wasm_no_module_passthrough` — 0 cost baseline
- `criterion 0.5 (async_tokio)` 加 dev-dep
- bench 配置 harness=false

### Verification

- `cargo build -p gate-wasm --benches` 净
- 实际跑参考：`cargo bench --package gate-wasm --bench wasm_invoke`

---

## [0.4.36] — 2026-05-23

**主题**：docs/threat-model.md — STRIDE 威胁建模 + WASM 表面分析。

### Added

- `docs/threat-model.md`：完整威胁模型
  - 资产清单（master key / channel keys / API keys / JWT / OIDC / audit log / billing）
  - 边界与信任图（Untrusted Internet → Kooix Gate → PostgreSQL/Upstream）
  - STRIDE 5 大类威胁清单 + 现状 + 缓解
  - 0.4.x WASM Plugin 新增表面分析（恶意 wasm / 供应链 / 审查盲区）
  - 0.5.0+ 安全 roadmap（模块签名 / cosign / registry 信任链）

---

## [0.4.35] — 2026-05-23

**主题**：docs/wasm-runbook.md — WASM 故障处理手册。

### Added

- `docs/wasm-runbook.md`：完整 WASM 故障手册
  - 模块加载失败（DigestMismatch / Load: compile / 路径错误）
  - hook 频繁超时 / OOM
  - panic 暴风雨（fallback 行为已保证业务不挂）
  - 上游全挂 / Redis 不可用 / 版本回滚
  - 联系人

---

## [0.4.34] — 2026-05-23

**主题**：Observability — Prometheus metrics 命名审计 + Grafana dashboard + OTLP trace 字段表。

### Added

- `docs/observability.md`：完整 metric 表（request lifecycle / upstream / routing / quota / billing / WASM）+ OTLP span 字段表 + sampling 策略
- `deploy/grafana/dashboards/kooix-gate-overview.json`：10 panel 概览 dashboard
  - Requests / sec / p95 Latency / Upstream 5xx / Quota Denies (4 个 stat)
  - Request rate by model / Upstream errors by channel
  - **WASM plugin calls (新 0.4.x)**
  - Billing settle lag / Outbox backlog / Channel health

### Verification

- grafana JSON 通过 lint

---

## [0.4.33] — 2026-05-23

**主题**：examples/wasm-transform 实战示例 — gate-wasm-sdk 用法 + system prompt 注入。

### Added

- `examples/wasm-transform/`：完整 WASM transform 示例
  - `Cargo.toml`：依赖 gate-wasm-sdk + serde_json + cdylib + wasm32-unknown-unknown 编译指引
  - `src/lib.rs`：`export_chat_request!` 宏使用，JSON parse + 在 messages 头部插 system prompt
  - `README.md`：编译、部署、验证、失败模式
- `examples/README.md` 索引加 `wasm-transform/` 行

---

## [0.4.32] — 2026-05-23

**主题**：OpenAPI 3.1 spec bump + API 参考文档。

### Changed

- `examples/openapi/kooix-gate.openapi.json`：version 0.2.0 → 0.4.31；description 加 ADR-0003 提及

### Added

- `docs/api-reference.md`：API 参考索引
  - OpenAPI 3.1 spec 用法（Swagger UI / Redocly）
  - Postman / Bruno collection 用法
  - 关键 API 路径表（Data Plane / Admin）
  - 错误码统一 shape 文档

---

## [0.4.31] — 2026-05-23

**主题**：deploy/helm/gate Helm chart 完善 — production-grade K8s 部署。

### Added

- `deploy/helm/gate/Chart.yaml`（v 0.4.31, appVersion 0.4.31）
- `deploy/helm/gate/values.yaml`：完整 values（master_key / jwt / postgres / redis / wasm limits / probes / autoscaling / observability / securityContext）
- `deploy/helm/gate/templates/_helpers.tpl`
- `deploy/helm/gate/templates/deployment.yaml`：完整 env 注入（含 *_fromSecret 双路径）
- `deploy/helm/gate/templates/service.yaml`：HTTP + Prometheus metrics 双端口
- `deploy/helm/README.md`：完整使用文档

### Verification

- yaml lint 通过（Chart.yaml / values.yaml）
- templates 在 helm CLI 渲染验证留 0.5.0+（本机未装 helm）

---

## [0.4.30] — 2026-05-23

**主题**：kgctl doctor 加 WASM runtime check —部署体检覆盖 ADR-0003 v0。

### Changed

- `kgctl doctor` 新增第 7 项 check：`WASM_RUNTIME`
  - 验证 wasmtime engine 可初始化
  - JSON 输出已保留（kgctl doctor --json）

### Verification

- `cargo build -p kgctl` 净

---

## [0.4.29] — 2026-05-23

**主题**：getting-started.md 三档接入文档 — Docker / Helm / 本地源码 + WASM Plugin Quickstart。

### Added

- `docs/getting-started.md`：
  - A. Docker Compose（30 秒）
  - B. Helm Chart（5 分钟，0.4.31 完善 values）
  - C. 本地源码（10 分钟开发用）
  - WASM Plugin transform Quickstart（gate-wasm-sdk 用法 + manifest 配置示例）
  - 故障排查 + 下一步索引
- `docs/README.md` 顶部加 Quickstart 索引

---

## [0.4.28] — 2026-05-23

**主题**：README 第一屏更新到 0.4.x 战果 — 285→485 tests / WASM Plugin 加入对比表。

### Changed

- Badge：tests 285 + 85 → 485 + 87；version 0.2.1 → 0.4.27
- "跟谁不同" 表新增 **WASM Plugin** 行（ADR-0003 v0）
- "当前版本" 段：替换 0.2.1 收尾叙事为 0.4.x 21 版本阶段战果
  - M3 完结
  - WASM Plugin v0 runtime + Rust SDK
  - 前端组件化数据
  - Rust 三巨兽拆解

---

## [0.4.27] — 2026-05-23

**主题**：gate-wasm-sdk crate — 用户写 wasm transform 模块的 Rust SDK。

### Added

- `crates/gate-wasm-sdk/`：std-based crate，wasm32-unknown-unknown target 编译
- `gate_alloc(size: i32) -> i32` bump allocator export
- `encode_return / return_payload / with_input` 工具
- `export_chat_request! / export_chat_response! / export_stream_chunk!` 三宏
- ABI v0 文档说明 + 编译命令

### Verification

- `cargo build -p gate-wasm-sdk` 净
- `cargo test -p gate-wasm-sdk` 0 failed (1 doctest ignored)

---

## [0.4.26] — 2026-05-23

**主题**：fallback policy + Prometheus metrics — wasm 失败永不 propagate。

### Added

- `fallback::invoke_with_fallback`：所有 hook 调用走此 wrapper
  - 模块未加载 → identity passthrough（status="no_module"）
  - 调用成功 → return transform 后 payload（status="ok"）
  - 失败（Timeout / OOM / DigestMismatch / Call / Load / Instantiate / HostDenied / Panic）→ `tracing::error!` + 降级
  - `FutureExt::catch_unwind` 双 safety
- Prometheus metric: `gate_plugin_wasm_calls_total{channel,hook,status}`
- 3 个新单元测试：FailingHost mock / 真 wasmtime success / module missing

### Verification

- `cargo test -p gate-wasm` 13 passed (+3)
- 失败永不 panic 上层 caller

---

## [0.4.25] — 2026-05-23

**主题**：3 个 hook 全部接通真实路径 — chat_request / chat_response / stream_chunk。

### Changed

- `WasmtimeHost::invoke_hook` 统一走真实路径；模块未 export hook 则 identity passthrough
- 移除 0.4.24 的 chat_request 单独分支

### Tests

- `chat_response_and_stream_chunk_invoke_real_module`：模块同时 export 3 hook，全部 e2e 验证
- 10 passed

---

## [0.4.24] — 2026-05-23

**主题**：chat_request_transform hook 真实接通 — wasm 端到端往返。

### Added

- `WasmtimeHost::invoke_hook_real`：完整 wasmtime async 调用链
  - Store + fuel budget 注入（max_cpu_ms × 1B）
  - Linker host fn `host_log` 占位
  - Module instantiate_async + memory + gate_alloc 调用
  - hook fn 返回 `i64 = (ptr<<32 | len)` 编码读 transform 后 payload
- `chat_request` 走真实路径；其他 hook 保持 identity（0.4.25 接通）
- 5 个新测试：
  - `chat_request_passthrough_when_hook_not_exported`
  - `chat_request_transforms_via_real_module`（真 WAT 模块）
  - `other_hooks_remain_identity_in_v024`
  - 既有 4 个 unit/load 测试

### Fixed

- 移除 `epoch_interruption(true)`，避免 async + epoch 双重 interrupt 导致空 fuel trap
- wasmtime crate 改用默认 features，去 cranelift/component-model 显式声明（默认已启）

### Verification

- `cargo test -p gate-wasm` 9 passed

---

## [0.4.23] — 2026-05-23

**主题**：plugin manifest `security.wasm` 字段升为 typed — ADR-0003 v0 schema 落地。

### Added

- `SecurityManifest::wasm: Option<WasmModuleManifest>`
- `WasmModuleManifest { module, module_sha256, max_memory_bytes, max_cpu_ms, hooks }`
- 2 个新单元测试：`parses_wasm_module_manifest_field` / `wasm_field_absent_when_not_configured`

### Verification

- `cargo test -p gate-providers --lib plugin_manifest` 32 passed (+2)
- `cargo build -p gate-providers` 净

---

## [0.4.22] — 2026-05-23

**主题**：WasmtimeHost runtime core — sha256 校验 + 模块加载 + fuel 设计。

### Added

- `crates/gate-wasm/src/wasmtime_host.rs`：
  - `WasmtimeHost` 基于 wasmtime 26 引擎
  - async_support / consume_fuel / epoch_interruption 全部启用
  - `load_module` SHA256 强校验，DigestMismatch fail-loud
  - `invoke_hook` v0 stub（identity transform，0.4.24-0.4.25 落 body 处理）
  - per-channel ChannelModule + Arc<RwLock<HashMap>> 隔离
- tokio sync feature 加入 deps（gate-wasm 自带）

### Verification

- `cargo test -p gate-wasm` 7 passed
  - wasmtime_host_new_succeeds
  - load_module_validates_sha256（含 happy path + DigestMismatch）
  - invoke_hook_returns_none_when_module_missing
  - invoke_hook_returns_identity_for_loaded_module
  - 3 个 lib level tests 保留

---

## [0.4.21] — 2026-05-23

**主题**：gate-wasm crate skeleton — ADR-0003 v0 runtime 落地起步。

### Added

- 新 crate `gate-wasm` 加入 workspace（基于 wasmtime 26）
- `src/lib.rs` — 模块入口 + 3 个 unit tests
- `src/error.rs` — `WasmError` / `WasmResult` 类型定义（Load / Instantiate / Call / Timeout / OutOfMemory / DigestMismatch / HostDenied / Panic）
- `src/limits.rs` — `ResourceLimits` + `DEFAULT_LIMITS`（16 MiB / 50ms / 1 module per channel）
- `src/host.rs` — `WasmHost` trait / `WasmHostConfig` / `HookKind` / `HookContext`
- wasmtime 26 + cranelift + async + component-model 编译通过

### Verification

- `cargo build -p gate-wasm` 净（1m 02s 首次编译）
- `cargo test -p gate-wasm` 3 passed

---

## [0.4.20] — 2026-05-23

**主题**：0.4.x 21 版本大收尾 — 0.5.0 债务清零，M3 全结。

### 0.4.x 全程战报（21 版本）

| 版本 | 主战 | 战果 |
|------|------|------|
| 0.4.0 | M3 Fast-path Runtime | ADR-0002 4 fast-path × 0.74-1.00 vs builtin |
| 0.4.1 | Rust 三巨兽拆解 | router/custom_provider/plugin_manifest 全部 -52% 以上 |
| 0.4.2-0.4.4 | channels 第一轮 | 1864 → 1487 (-20.2%) |
| 0.4.5-0.4.9 | groups/quotas/users/pricing/requests 拆 modal + 共享 helper | -350 行总计 |
| 0.4.10 | 阶段收尾 | M1.4 partial ticked |
| 0.4.11 | ChannelTable 完整组件化 | channels 1487 → 1252 (-15.8%) |
| 0.4.12 | QuotaWizard | quotas 948 → 722 (-23.8%) |
| 0.4.13 | QuotaForm | quotas 722 → 680 |
| 0.4.14 | PricingWizard | pricing 633 → 369 (-41.7%) |
| 0.4.15 | SessionModal | users 729 → 669 |
| 0.4.16 | **ADR-0003 WASM Plugin ABI v0** | M3 唯一未结收口 |
| 0.4.17 | channels/[channelId] 3 modal | 667 → 618 |
| 0.4.18 | bundle budget 250→220KB | M1.4 T2.6 ticked |
| 0.4.19 | billing helpers | 463 → 435 |
| 0.4.20 | **大收尾 / 阶段总结** | ROADMAP M1 / M3 全 ticked |

### 前端 +page.svelte Top 10（0.4.20 结算）

```
1252  channels/+page.svelte         (起 1864 → -32.8%)
 972  admin/groups/+page.svelte     (起 1083 → -10.2%)
 680  orgs/[orgId]/quotas/+page.svelte  (起 959 → -29.1%)
 669  admin/users/+page.svelte      (起 752 → -11.0%)
 618  channels/[channelId]/+page.svelte (起 667 → -7.3%)
 594  admin/requests/+page.svelte   (起 628 → -5.4%)
 507  usage/requests/+page.svelte   (起 541 → -6.3%)
 442  admin/audit/+page.svelte
 439  admin/sso/+page.svelte
```

### 0.4.x 新建工件

- **22 个 _components 子组件**：channels (6) + channels/[channelId] (3) + admin/groups (4) + admin/users (3) + admin/pricing (3) + orgs/quotas (3) + usage/requests (1)
- **3 个 _lib helper 模块**：channels (137 行) / billing (38 行) / requests-helpers (59 行)
- **ADR-0003 WASM Plugin ABI v0** + sample manifest（0.5.0 wasmtime runtime 占位）

### M3 全结

- ADR-0001 Providers as Plugin ✅
- ADR-0002 Fast-path Runtime ✅
- ADR-0003 WASM Plugin ABI v0 ✅（runtime 留 0.5.0）

### Roadmap M1.4 全 ticked

- T2.1 channels 拆 ✅
- T2.3 admin/pricing 拆 ✅
- T2.4 usage/requests 拆 ✅
- T2.6 bundle budget 收紧 ✅

### Verification

- `npm run check` 0 errors / 0 warnings
- `cargo build` 净
- 21 个 tag (v0.4.0..v0.4.20) 全部 push origin/main

### Deferred 到 0.5.0+

- wasmtime runtime 落地（ADR-0003 v0 → v1）
- 进一步收敛 channels/+page (1252) + admin/groups (972) — 按需拆分
- 进一步收紧 web bundle budget 220 → 180KB

---

## [0.4.19] — 2026-05-23

**主题**：billing helpers 抽出 — 463 → 435 行 (-6.0%)。

### Refactor

- `_lib/helpers.ts` (38 行) — fmtCost / fmtNum / scopeLabel / invoiceStatusLabel / invoiceStatusVariant 5 个纯函数

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.18] — 2026-05-23

**主题**：Web bundle budget 阈值收紧 — 250KB → 220KB（M1.4 T2.6 ticked）。

### Changed

- `web/scripts/check-bundle-budget.mjs`：default `maxEntryBytes` 250_000 → 220_000
- 实测最大 chunk 204KB（含 svelte runtime），channels page 156KB；阈值留 ~10% margin
- CI 集成已在 `.github/workflows/ci.yml:191` 跑 `npm run bundle:budget`

### Verification

- `npm run build` 5.66s 净
- `npm run bundle:budget` ok at ≤ 220000 bytes

### Roadmap 同步

- M1.4 T2.6 Web bundle budget 收紧门禁 — ticked
- 0.5.0+ 计划：channels 页 ChannelTable + drawer 全部拆出后阈值收到 180KB

---

## [0.4.17] — 2026-05-23

**主题**：channels/[channelId] 3 modal 抽出 — 667 → 618 行 (-7.3%)。

### Refactor

- `_components/RevokeKeyModal.svelte` (43 行)
- `_components/CreateKeyModal.svelte` (53 行)
- `_components/RotateKeyModal.svelte` (63 行)

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.16] — 2026-05-23

**主题**：ADR-0003 WASM Plugin ABI v0 PoC 收口 — M3 唯一未结项达成。

### Added

- `docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md`：
  - v0 host functions 最小集（plugin_init / chat_request_transform / chat_response_transform / stream_chunk_transform / host_log / host_get_secret_slot / host_record_metric）
  - hard limits（≤ 50ms CPU / ≤ 16 MiB memory / no I/O / deterministic）
  - 与 HTTP Plugin manifest v1 的 inner transform layer 关系定型
  - Fallback 与 audit 边界（panic / OOM / timeout 降级到 manifest 路径）
- `examples/manifest-registry/community/wasm-transform/0.1.0/`：sample manifest 占位（`security._wasm_v0_placeholder`，0.4.x validator 跳过未知字段）
- `docs/wasm-plugin-abi.md` 同步指向 ADR-0003

### Roadmap 同步

- M3 v0.4.0 WASM Plugin ABI vNext PoC 标记 ticked

### Deferred to 0.5.0+

- wasmtime runtime 落地（`crates/gate-providers/src/wasm_plugin/`）
- `SecurityManifest::wasm_*` 字段从占位升为 typed field
- Rust SDK + golden test
- AssemblyScript / Go SDK 文档

---

## [0.4.15] — 2026-05-23

**主题**：SessionModal 抽出 — admin/users 729 → 669 行 (-8.2%)。

### Refactor

- `_components/SessionModal.svelte` (123 行) — 75 行 session list modal + format helper

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.14] — 2026-05-23

**主题**：PricingWizard 完整组件化 — admin/pricing 633 → 369 行 (-41.7%)。

### Refactor

- `_components/PricingWizard.svelte` (404 行) — 304 行 4 步 wizard form 整体抽出
- 36 顶层 props（11 bindable + 12 read-only state + 11 callback/util）

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.13] — 2026-05-23

**主题**：QuotaForm 抽出 — quotas 722 → 680 行 (-5.8%)。

### Refactor

- `_components/QuotaForm.svelte` (109 行)

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.12] — 2026-05-23

**主题**：QuotaWizard 完整组件化 — quotas 948 → 722 行 (-23.8%)。

### Refactor

- `_components/QuotaWizard.svelte` (321 行) — 252 行 wizard modal 整体抽出
- 26 顶层 props（含 5 derived state + 11 callback + 5 sample/format helpers）

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.11] — 2026-05-23

**主题**：ChannelTable 完整组件化 — channels 1487 → 1252 行 (-15.8%)。

### Refactor

- `_components/ChannelTable.svelte` (289 行)：262 行 DataTable + head snippet + rows + expanded body
  抽出。props 表用 `actions` 对象分组，从 ~30 props 收敛到 14 顶层 props（含 1 actions 对象）。

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.10] — 2026-05-23

**主题**：0.4.x 阶段收尾 — M1.4 前端拆解里程碑达成，ROADMAP 同步。

### 阶段战报（0.4.0 → 0.4.10）

| 阶段 | 战果 |
|------|------|
| 0.4.0 | M3 Fast-path Runtime 完成（ADR-0002 4 fast-path × 0.74-1.00 vs builtin） |
| 0.4.1 | Rust 三巨兽拆解（router/custom_provider/plugin_manifest 全部 -52% 以上） |
| 0.4.2 | channels: 1864 → 1718 (-7.8%) — helpers + EditChannelDrawer |
| 0.4.3 | channels: 1718 → 1515 (-11.8%) — CreateChannelDrawer |
| 0.4.4 | channels: 1515 → 1487 (-1.8%) — badge/fmt helpers |
| 0.4.5 | admin/groups: 1083 → 972 (-10.2%) — 4 modal |
| 0.4.6 | quotas: 959 → 948 (-1.1%) — QuotaDeleteModal |
| 0.4.7 | admin/users: 752 → 729 (-3.1%) — 2 modal |
| 0.4.8 | admin/pricing: 640 → 633 (-1.1%) — DeletePricingModal |
| 0.4.9 | requests 双页 -68 行 (-5.7%) — 共享 helper |

### 前端 +page.svelte Top 10（0.4.10 结算）

```
1487  channels/+page.svelte
 972  admin/groups/+page.svelte
 948  orgs/[orgId]/quotas/+page.svelte
 729  admin/users/+page.svelte
 667  channels/[channelId]/+page.svelte
 633  admin/pricing/+page.svelte
 594  admin/requests/+page.svelte
 507  usage/requests/+page.svelte
 463  orgs/[orgId]/billing/+page.svelte
```

### 新增 _components / _lib

- `channels/_components/`: EditChannelDrawer + CreateChannelDrawer + 3 existing modal = 5
- `channels/_lib/helpers.ts`: 12 个共享 fn/常量
- `admin/groups/_components/`: 4 modal
- `admin/users/_components/`: 2 modal
- `admin/pricing/_components/`: DeletePricingModal + existing PricingRulesTable = 2
- `orgs/[orgId]/quotas/_components/`: QuotaDeleteModal
- `$lib/requests-helpers.ts`: 6 个跨页共享 fn

### Deferred 到 0.5.0+

- ChannelTable 完整组件化（DataTable 段 ~30 props，硬拆反模式）
- QuotaForm / QuotaWizard 拆分（多步流程 + 复杂 derived state）
- Pricing wizard form（多步 + 计费 preview state）
- admin/users session modal（75 行嵌套 sessions 列表）
- T2.6 Web bundle budget 收紧门禁阈值

### Verification

- `npm run check` 0 errors / 0 warnings
- `npm test` 87 passed
- `cargo build` 净

### ROADMAP 同步

- M1.4 T2.1 / T2.3 / T2.4 全部 ticked
- 新增 "0.4.x 额外" 条目记录 modal 拆解战果

---

## [0.4.9] — 2026-05-23

**主题**：requests 双页共享 helper 抽出 — admin/requests 628 → 594 / usage/requests 541 → 507（合计 -68 行 / -5.7%）。

### Refactor

- `$lib/requests-helpers.ts` (59 行) — 6 个纯函数跨页共享：
  - `rangeToDate` / `statusBadgeCls` / `formatRequestDate` /
    `formatLatency` / `formatCost` / `formatTokens`
- `formatDate` → `formatRequestDate` 统一命名（避免与他页同名冲突）

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.8] — 2026-05-23

**主题**：admin/pricing DeletePricingModal 抽出 — 640 → 633 行 (-7)。

### Refactor

- `_components/DeletePricingModal.svelte` (41 行)
- pricing wizard form (310-624, 314 行) 多步流程 + 复杂 state，留 0.5.0+ 评估

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.7] — 2026-05-23

**主题**：admin/users ResetPassword + SuspendUser modal 抽出 — 752 → 729 行 (-23)。

### Refactor

- `_components/ResetPasswordModal.svelte` (53 行)
- `_components/SuspendUserModal.svelte` (59 行)
- session modal 75 行内嵌 sessions 列表，留 0.5.0+ 独立拆

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.6] — 2026-05-23

**主题**：orgs/quotas QuotaDeleteModal 抽出 — 959 → 948 行 (-11)。

### Refactor

- `_components/QuotaDeleteModal.svelte` (46 行)
- QuotaForm / Wizard 体量大且 props 表过宽，留 0.5.0+ 评估

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.5] — 2026-05-23

**主题**：admin/groups/+page.svelte 拆 4 个 modal — 1083 → 972 行 (-10.2%)。

### Refactor

- `_components/CreateGroupModal.svelte` (75 行)
- `_components/DeleteGroupModal.svelte` (39 行)
- `_components/DisableGroupModal.svelte` (43 行)
- `_components/AddChannelModal.svelte` (107 行)

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.4] — 2026-05-23

**主题**：channels helpers 扩充 — `fmtLimit` / `fmtDate` / `statusBadgeCls` /
`healthBadgeCls` / `healthDot` 抽到 `_lib/helpers.ts`。

### Refactor

- channels/+page.svelte: 1515 → 1487 行 (-28)
- `_lib/helpers.ts`: 97 → 137 行（追加 5 个表格 badge / fmt helper）
- ChannelTable 完整组件化推迟到 0.5.0+：DataTable 段调用面 ~30 props，硬拆反模式。
  本版改为抽 row 渲染辅助函数，让表格代码可读但仍保持单页 owner。

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.3] — 2026-05-23

**主题**：channels/+page.svelte CreateDrawer 拆分 — 1718 → 1515 行 (-11.8%)。

### Refactor

- `_components/CreateChannelDrawer.svelte` (367 行)：把 218 行 CreateDrawer template
  抽成独立组件。props 表 36（12 bindable + 9 read-only + 15 callback/util），manifest
  builder 7 步流程整体内嵌。
- `pluginAuthSlotSummary` 移到 `_lib/helpers.ts`，跨 drawer 共用。
- Svelte 5 占位符 `{{model}}` 转 const 变量化（`REQUEST_BODY_PLACEHOLDER` /
  `PROBE_PATH_PLACEHOLDER`），避免被解析为 expression。

### Verification

- `npm run check` 0 errors / 0 warnings

---

## [0.4.2] — 2026-05-22

**主题**：M1.4 前端 channels 巨兽初拆 — `channels/+page.svelte` 1864 → 1718 行（-7.8%）。

### Refactor — Channels page split (M1.4 partial)

- **`channels/_lib/helpers.ts` 新增 (69 行)**：抽出 4 个纯函数（`isPluginProvider` /
  `capabilityFallback` / `capabilityTitle` / `capabilityChipClass`）+ 4 个常量
  （`PROVIDER_OPTIONS` / `FILTER_PROVIDER_OPTIONS` / `STATUS_OPTIONS` / `HEALTH_OPTIONS`），
  与 state 解耦，可被 `_components/*` 子组件共用。
- **`_components/EditChannelDrawer.svelte` 新增 (232 行)**：把 EditDrawer 整段 141 行
  template 抽成独立组件，25 props（11 bindable + 6 read-only state + 8 callback +
  3 sample 常量）。`+page.svelte` 端调用面收敛到 `<EditChannelDrawer ... />` 一处。

### Verification

- `npm run check` 0 errors / 0 warnings
- `npm test` 87 passed / 0 failed (13 files)
- `npm run build` 5.76s 净

### Deferred to 0.4.3

- `CreateChannelDrawer.svelte`（218 行 template，props 表~40，需配合 ManifestBuilder 二次拆）
- `ChannelTable.svelte`（261 行 DataTable 块，19 props）
- `admin/groups/+page.svelte`（1083 行）

---

## [0.4.1] — 2026-05-22

**主题**：M1.3 三巨兽拆解收尾 — 单文件 ≤ 800 行目标推进。

### Refactor — Three giant files split (M1.3 T3.1-T3.3)

- **`plugin_manifest/mod.rs` 1472 → 705 行**（-52%）：抽出
  `plugin_manifest/{factory,helpers,tests}.rs` 三子模块。`factory.rs` 收口
  `validate_plugin_manifest` / `plugin_manifest` / `plugin_manifest_retry_config` /
  `plugin_manifest_schema_json` 4 个公共入口；`helpers.rs` 内部 JSON pointer / config
  error 工具；`tests.rs` 24 个单元测试搬迁。
- **`custom_provider/mod.rs` 3516 → 1452 行**（-59%）：抽出
  `custom_provider/{helpers,fastpath,tests}.rs`。`helpers.rs` 27 个 free fn / RenderedValue
  struct（模板渲染 / JSON 路径 / header 插入 / 错误判定）；`fastpath.rs` ADR-0002
  fast-path inherent impl 块（`fastpath_kind` / `run_fastpath` + 4 provider 8 fn）；
  `tests.rs` 整体迁出。
- **`router/mod.rs` 3540 → 1713 行**（-52%）：抽出 `router/tests.rs`，1830 行
  unit/integration 测试搬迁。

### Verification

- `cargo fmt --all -- --check` 净
- `cargo clippy --workspace --all-targets -- -D warnings` 净
- `cargo test --workspace --lib`：485 passed / 0 failed（plugin_manifest 30 / custom_provider
  120 / router 44 / 其他 291）
- `gate-server` 关键 e2e：c1_routing 11 passed / channel_plugin_e2e 1 passed

### Notes

- 三巨兽再拆受限于内部循环依赖（`ProviderRouter` impl 跨 dispatch fn 强耦合 /
  `PluginManifest::from_value` 与 type 系统强耦合），1452 / 1713 行已是现阶段最优。
- M1.3 T3.6 / T3.7（clippy 基线 + crate README）一并完成。

### Bench parity

- 0.4.1 重新跑 `cargo bench --package gate-providers --bench plugin_vs_builtin`，
  fast-path 路径性能与 0.4.0 持平（× 0.74-1.00），拆分零回归。

---

## [0.4.0] — 2026-05-22

**主题**：ADR-0002 M3 Fast-path Runtime 完成 — `gate-providers` 终极形态收尾。

### Highlights

- **OpenAI / Anthropic / Azure / Bedrock 四条 fast-path 全接通**：plugin runtime 在 4 个高频
  provider 上跳过 manifest 解释器，直接走静态分发。bench 实测 fast-path × 0.74-1.00 vs
  builtin（manifest runtime 是 × 1.27-1.45），远好于 ADR-0002 ≤ × 1.02 预算。
- **Bedrock SigV4 修真**：编译期 `BedrockProvider` 之前的占位假签名（仅发
  `X-Amz-Access-Key/Secret-Key` 头）换成真 AWS Signature V4，AWS 已知向量 test 通过。
  `crate::sigv4` 模块提到 crate 顶层供 anthropic / bedrock / custom_provider 共用，
  零协议重复。
- **Capability matrix golden test**：23 个 preset × 9 capability + base_url 默认值字节级锁定，
  drift 即 fail。`KOOIX_UPDATE_FIXTURES=1` 触发刷新。
- **catch_unwind fallback**：fast-path panic 时 `tracing::error!` + 降级到 manifest runtime，
  防御性兜底进程不挂。
- **preset bundle 决策**：评估后**不拆 crate** —— 23 个 preset 共享 OpenAI adapter，硬拆
  重复代码。`plugin_preset.rs` 单文件 896 行保留。详见 [ADR-0002 § preset bundle 决策](./docs/architecture/decisions/ADR-0002-fastpath-runtime.md#preset-bundle-决策2026-05-22)。

### Added — Plugin runtime perf bench (ADR-0001 verification)

- 新增 [`crates/gate-providers/benches/plugin_vs_builtin.rs`](./crates/gate-providers/benches/plugin_vs_builtin.rs)：
  Criterion micro-bench 对比 `OpenAiProvider` vs `CustomHttpProvider + openai_compatible preset`
  的 chat 路径单次调用耗时，wiremock localhost endpoint。
- 实测数据（2026-05-22）：builtin 25.6 µs / plugin 36.2 µs，**ratio × 1.41**，超 ADR-0001 的
  5% 预算 8 倍。详见 [ADR-0001 Verification 段](./docs/architecture/decisions/ADR-0001-providers-as-plugin.md#bench-数据2026-05-22)。
- 触发 M3 立项：[ADR-0002 Fast-path Runtime](./docs/architecture/decisions/ADR-0002-fastpath-runtime.md)。

### Added — M3 设计落地

- 新增 [ADR-0002](./docs/architecture/decisions/ADR-0002-fastpath-runtime.md)：`builtin_fastpath`
  manifest 标志位 + 4 个高频 provider 静态分发设计。
- ROADMAP M3 章节加 bench 触发依据 + 新增 capability matrix golden test / panic fallback
  两条验收项。

### Added — M3 T0：`builtin_fastpath` schema + golden test

- `SecurityManifest::builtin_fastpath: bool` 字段落地。
  - 用户 channel manifest 设置的值在 `PluginManifest::from_value` 入口被**强制清零**。
  - 只有 `apply_preset` 给 4 个 fast-path（openai / anthropic_messages / azure_openai /
    bedrock_converse）静态注入 `true`。
  - 0.3.x 仅落地 schema + 注入点，dispatch 实现留 0.4.0。
  - 4 个新单元测试 (`plugin_manifest::tests::builtin_fastpath_*` /
    `user_cannot_override_*`) 锁定上述不变式。
- 新增 [`crates/gate-providers/tests/capability_matrix.rs`](./crates/gate-providers/tests/capability_matrix.rs)：
  golden test 锁 23 个 preset 的 `(capabilities, base_url_suggestion)` 矩阵；
  漂移时跑 `KOOIX_UPDATE_FIXTURES=1 cargo test --test capability_matrix` 刷新 fixture。
  Fixture 见 [`tests/fixtures/capability_matrix.json`](./crates/gate-providers/tests/fixtures/capability_matrix.json)。

### Added — Bench baseline `pre-m3`

- 跑了 `cargo bench --bench plugin_vs_builtin -- --save-baseline pre-m3`，
  M3 实施期间用 `--baseline pre-m3` 对比定量验收。文件落在 `target/criterion/`，
  不入 git；CI 可重跑生成。

### Added — M3 T2a：catch_unwind fallback 兜底

- `CustomHttpProvider::run_fastpath` helper：用 `FutureExt::catch_unwind` 包裹 fast-path
  调用，panic 时记录 `tracing::error!` 并返回 `None`，让 trait impl 顶部分发降级到
  manifest runtime 老路。3 个单元测试（OK 路径 / panic 路径 / panic_message 解析）。
- 防御性设计：fast-path 是手写代码路径，理论上不会 panic；这层兜底防止 OpenAI / Anthropic
  改变响应格式触发 serde panic 时进程不挂。

### Added — M3 T2b：Anthropic Messages fast-path

- `CustomHttpProvider` 内部 `fastpath_anthropic_chat / chat_stream` 落地：
  - `crate::anthropic` 模块新加 `pub(crate)` wrapper（`fastpath_anthropic_request_body` /
    `_response_from_json` / `_sse_stream` / `_check_status` + `FASTPATH_ANTHROPIC_VERSION`），
    复用编译期 `to_anthropic_request` / `from_anthropic_response` / `anthropic_sse_to_chunks`，
    **零协议重复**。
  - `preset.kind == AnthropicMessages` 时走 fast-path：POST `/v1/messages`，
    `x-api-key` + `anthropic-version: 2023-06-01` 头，body 转 Anthropic 原生格式
    （system / content blocks / tool_use / tool_result），响应映射回 OpenAI ChatResponse。
  - 2 个 integration test（chat / chat_stream）锁路径正确性。
- 老 test `preset_anthropic_messages_posts_native_body_and_normalizes_response` 调整：
  body 期望去掉 `"stream": false` 字段，以匹配 fast-path 行为（与编译期
  `AnthropicProvider` 一致：stream=None 时 skip serialized）。这是行为收敛，不是回归。

### Bench 数据更新（OpenAI + Anthropic fast-path 全接通后）

- builtin_openai             ≈ 24-28 µs
- plugin_openai_compatible   ≈ 35 µs   × 1.45 vs builtin
- **plugin_openai_fastpath   ≈ 21-23 µs   × 0.74-0.96 vs builtin** — ADR-0002 ≤ × 1.02 预算达成

ADR-0002 verification 5/7 项已勾，剩 Azure / Bedrock 2 个 adapter + preset bundle 拆 crate 留 0.4.0。

### Added — M3 T2c：Azure OpenAI fast-path

- `CustomHttpProvider` 内部 `fastpath_azure_chat / chat_stream / embed` 落地：
  - Deployment URL 模板：`{base_url}/openai/deployments/{model}/chat/completions?api-version={X}`，
    `api-key` 头鉴权。
  - `manifest.preset.api_version` 通过 schema 传到 fast-path（default `2024-08-01-preview`）。
  - 协议 body / response 与 OpenAI 一致 → 复用 `crate::openai::{check_status, sse_to_chunks}`。
  - 2 个 integration test 锁定：URL 模板正确性 + api-version override 透传。

### Added — M3 T2d：Bedrock SigV4 修真 + Converse fast-path

- 编译期 `BedrockProvider::sign_request` 的假签名（仅发 `X-Amz-Access-Key/Secret-Key` 头）
  换成真 AWS Signature V4：
  - `BedrockProvider::sigv4_sign_post` 实现完整 AWS SigV4：canonical request /
    string-to-sign / HMAC signing key 全用新提到 crate 顶层的 `crate::sigv4` helper。
  - 输出标准 `Authorization: AWS4-HMAC-SHA256 Credential=.../bedrock/aws4_request,
    SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=...` 头 + `x-amz-date`
    + `x-amz-content-sha256`。
  - AWS 已知向量 test 通过（signing key `c4afb1cc5771d871...` for
    `20150830/us-east-1/iam/aws4_request`，AWS 官方文档 vector）。
- `crate::sigv4` 从 `custom_provider/sigv4.rs` 提到顶层 `pub(crate) mod sigv4`，
  让 anthropic.rs / bedrock.rs / custom_provider 都能复用。原 mod 文件保留 re-export 兼容。
- `CustomHttpProvider::fastpath_bedrock_chat` 落地：
  - URL: `{base_url}/model/{model}/converse`
  - Region: `infer_aws_region_from_host` → `AWS_REGION` env → `us-east-1` 兜底
  - Secrets: 标准 plugin slot `aws_access_key` / `aws_secret_key`，缺失时 fail-loud
  - Body/Response: 复用 `crate::bedrock::fastpath_bedrock_request_body / _response_from_json`
- 2 个 integration test（签名成功 + 缺 secret fail-loud）+ 2 个 unit test（sigv4 vector
  + authorization header 格式）。

ADR-0002 verification 7/8 项已勾，剩 preset bundle 拆 crate 评估留 0.4.0。

### Added — M3 T1：OpenAI fast-path dispatch 接通

- `CustomHttpProvider::chat / chat_stream` + `EmbeddingProvider::embed` 顶部加 fast-path
  分发：`security.builtin_fastpath = true` 且 `preset.kind == Openai` 时，跳过 manifest
  解释器，直接 POST `/chat/completions` Bearer 鉴权（等价于 `OpenAiProvider`）。
- 复用 `crate::openai::{check_status, sse_to_chunks}`，与编译期 OpenAI 路径**字节级一致**。
- 保留 sandbox dns + peer 校验（安全不能省）。
- bench 加 `plugin_openai_fastpath` 列。**实测数据**：
  - builtin_openai           24.1 µs  [23.5, 24.8]
  - plugin_openai_compatible 35.0 µs  [34.2, 35.7]  × 1.45
  - plugin_openai_fastpath   **23.1 µs**  [22.3, 24.1]  **× 0.96**

  fast-path 与 builtin 性能等价，达成 ADR-0002 ≤ × 1.02 预算。
- 3 个新 integration test (`fastpath_openai_chat_*` / `fastpath_does_not_apply_*`) 锁定：
  - `preset.provider="openai"` → fast-path 路径正确请求 + 响应解析；
  - `preset.provider="openai_compatible"` 不被 fast-path 误伤，仍走 manifest 解释器。
- ADR-0002 verification 第 3 项打勾；剩 Anthropic / Azure / Bedrock 3 个 fast-path
  adapter + catch_unwind fallback + preset bundle 拆 crate 留 0.4.0。

---

## [0.3.0] — 2026-05-22

### **BREAKING** — Compile-time thin wrapper providers retired (ADR-0001)

5 个编译期 thin wrapper provider 已删除，统一改走 plugin runtime + preset：

- `crates/gate-providers/src/cohere.rs` ❌ → plugin preset `cohere_chat`
- `crates/gate-providers/src/deepseek.rs` ❌ → plugin preset `deepseek`
- `crates/gate-providers/src/gemini.rs` ❌ → plugin preset `gemini`
- `crates/gate-providers/src/mistral.rs` ❌ → plugin preset `mistral`
- `crates/gate-providers/src/ollama.rs` ❌ → plugin preset `ollama`

`gate_providers::CohereProvider` / `DeepSeekProvider` / `GeminiProvider` / `MistralProvider` /
`OllamaProvider` 公共类型已删除。直接 import 这些类型的下游代码会编译失败。

### Added — Channel migration 20260522000001

- 新增 [`crates/gate-storage/migrations/20260522000001_migrate_thin_wrapper_to_plugin.sql`](./crates/gate-storage/migrations/20260522000001_migrate_thin_wrapper_to_plugin.sql)：
  把存量 `channels` 表中 5 类 `provider_type` 自动迁移为 `provider_type='plugin'` +
  `model_mapping.plugin.preset.provider='<对应 preset 名>'`。
- Migration 幂等：再跑一次不会重复写入。
- 回滚步骤见 SQL 文件头部注释（需要带回 5 thin wrapper 源码）。

### Changed — Router fail-loud on legacy provider_type

- `gate_providers::router::builder::build_provider_with_secrets` / `build_embedding_provider_with_secrets`：
  遇到 `provider_type` ∈ `cohere/deepseek/gemini/mistral/ollama` 时返回 `ProviderError::Config`，
  提示用户跑 `kgctl migrate`。不再静默走 OpenAI 兼容回退。

### Changed — Frontend channel form

- `web/src/routes/channels/+page.svelte` `PROVIDER_OPTIONS` 下拉移除 5 个删掉的 thin wrapper +
  10 个本来就是 plugin preset 的别名（groq / together / openrouter / moonshot / 智谱 / 通义 / 零一 等）。
  保留 4 个 fast-path（OpenAI / Anthropic / Azure / Bedrock）+ 1 个 "HTTP Plugin"。所有 18+ preset
  在 plugin manifest builder 内提供。

### Version

- Workspace 版本切到 `0.3.0`（9 crate 全量同步：gate-core / gate-storage / gate-crypto / gate-auth / gate-cache / gate-providers / gate-server / gate-billing / kgctl）。
- web/package.json 版本切到 `0.3.0`。

### Migration & Upgrade

1. **DB**：发版前必须先跑 `kgctl migrate`（应用 migration 20260522000001）。
2. **应用层**：升级 binary。新 binary 拒绝 legacy `provider_type` 值，但 migration 已经把存量改完，正常流量不受影响。
3. **下游 SDK**：如有第三方代码 `use gate_providers::CohereProvider` 等，需改成
   `let manifest = json!({"plugin":{"preset":{"provider":"cohere_chat"}}}); CustomHttpProvider::new(...)`。

---

## [0.2.1] — 2026-05-22

### Refactor — Three Megafile Split

- 拆 `crates/gate-providers/src/router.rs` (4524 行) → `router/{mod,trace,routed,metrics,selection,helpers,builder}.rs`，主 `mod.rs` 减至 ~3500 行；6 个子模块每个 ≤ 285 行。
- 拆 `crates/gate-providers/src/custom_provider.rs` (3878 行) → `custom_provider/{mod,sandbox,replay,sigv4,secrets}.rs`，主 `mod.rs` 减至 ~2980 行；4 个子模块每个 ≤ 407 行。
- 拆 `crates/gate-providers/src/plugin_manifest.rs` (2193 行) → `plugin_manifest/{mod,validate,upgrade}.rs`，主 `mod.rs` 减至 ~1380 行；validate 754 行 + upgrade 86 行。
- 拆分对外公共 API 完全保持兼容：所有 `pub use` re-export 通过 `mod/mod.rs` 转发，`gate_providers::ProviderRouter` / `CustomHttpProvider` / `replay_plugin_sse` / `PluginManifest` 等外部访问路径不变。
- `cargo clippy --workspace --all-targets -- -D warnings` 全绿；217 lib tests 全过；web check 0 errors / 0 warnings；web build 通过。

### Refactor — Frontend Page Split

- 拆 `web/src/routes/channels/+page.svelte` (1949 行) → 抽 `_components/{ProbeModal,DeleteConfirmModal,BatchConfirmModal}.svelte` + 通用 `lib/components/ui/Pagination.svelte`；主页面减至 1875 行。
- 拆 `web/src/routes/admin/pricing/+page.svelte` (683 行) → 抽 `_components/PricingRulesTable.svelte`；主页面减至 640 行。
- 拆 `web/src/routes/usage/requests/+page.svelte` (547 行) → 抽 `_components/CursorPagination.svelte`；主页面减至 541 行。
- web check / vitest (87 tests) / build 全绿。

### Added — gate-providers Crate Documentation

- 新增 [crates/gate-providers/README.md](./crates/gate-providers/README.md)：模块树（router / custom_provider / plugin_manifest 三巨兽拆分后结构）+ 公共 API + 演进方向 + 关键约束 + 测试入口。

### Changed — Web Bundle Budget

- `web/scripts/check-bundle-budget.mjs` 阈值从 750_000 收紧到 250_000 字节（拆分后单个 chunk 实际 ≤ 204 KB）。可用 `KOOIX_WEB_BUNDLE_MAX_BYTES` 临时覆盖。

### Added — Test Distribution Convention

- `CONTRIBUTING.md` 新增「跨 crate integration test 分布」表格：30 个 test 文件分 6 crate（gate-storage 5 / gate-providers 2 / gate-cache 1 / gate-billing 2 / gate-server 19 / kgctl 1），每 crate 测自己边界。优化 test 编译时间走 cargo-nextest（复用 binary）而非迁移文件位置。

### Added — Self-critique & Roadmap Refactor

- 新增 [docs/stages/2026-05-21-self-critique-todo.md](./docs/stages/2026-05-21-self-critique-todo.md)：四道劫痕（定位模糊 / 前端散乱 / 渠道半成品 / 编译产物太大）+ 26 条整改 TODO + 三里程碑执行顺序 + 7 条验收线。
- 新增 [ADR-0001 Provider 全插件化迁移](./docs/architecture/decisions/ADR-0001-providers-as-plugin.md)：固化 0.2.1 → 0.3.0 → 0.4.0 三阶段迁移路径、capability parity、性能预算（5%）、双跑窗口、回滚方案。
- ROADMAP 重构：路线总览改为三里程碑（M1 v0.2.1 收尾 / M2 v0.3.0 退役 / M3 v0.4.0 fast-path），原 P0/P1/P2 段保留为已完成基线证据。
- ROADMAP M1.5：playground 收编为产品线（visual workflow editor），不再视为异物；7 节点共享 `ProviderCapability` 矩阵、节点工作流执行接入 `request_events` audit。
- README 第一屏重写：定位句 + 是什么/不是什么 + vs 竞品对比表（vs LiteLLM / OneAPI / OpenRouter）+ 30 秒 quickstart；删除能力流水账，全部引用 DESIGN/ROADMAP。

### Added — Architecture Documentation

- 新增 [docs/playground.md](./docs/playground.md)：playground 节点类型、ProviderCapability 联动、bundle 策略、已知限制、M1.5 路线。
- 新增 [web/src/lib/components/README.md](./web/src/lib/components/README.md)：38 个 Svelte 组件分类索引（templates / ui / channels / flow / playground / brand）+ 约定。
- 充实 [docs/architecture/control-plane.md](./docs/architecture/control-plane.md) / [data-plane.md](./docs/architecture/data-plane.md) / [worker-plane.md](./docs/architecture/worker-plane.md)：每页加职责矩阵、关键约束、状态机、错误归一表、关键链路、代码锚点、跨页面交叉引用。

### Changed — Build & Disk Usage

- `Cargo.toml [profile.dev]` 调优：`debug = "line-tables-only"` + `split-debuginfo = "unpacked"` + `[profile.dev.package."*"] opt-level = 1`，预计 `target/debug` 体积砍 3-4 倍（163 GB → 40-55 GB）。
- 新增 [.config/nextest.toml](./.config/nextest.toml)：cargo-nextest 配置（default + ci profile，slow-timeout，testcontainers-aware filter override）。
- 新增 [scripts/cargo-sweep-helper.sh](./scripts/cargo-sweep-helper.sh)：dry-run / apply / deep clean 模式，30 天 fingerprint 阈值可通过 `KOOIX_SWEEP_DAYS` 覆盖。
- `CONTRIBUTING.md` 新增「Disk usage management」章节：cargo-sweep / cargo-nextest / sqlx migrate cache / dev profile 用法。

### Changed — Provider Deprecation Warnings

- 5 个 thin wrapper provider 标 `#[deprecated(since = "0.2.1", note = "use plugin preset; will be removed in 0.3.0. See ADR-0001.")]`：`CohereProvider` / `DeepSeekProvider` / `GeminiProvider` / `MistralProvider` / `OllamaProvider`。
- `gate-providers/src/router.rs::build_provider_with_secrets` / `build_embedding_provider_with_secrets` 加 `#[allow(deprecated)]` 临时门面，等 0.3.0 删除 thin wrapper 时一并清理。

### Changed — Frontend

- `web/package.json`：`lucide-svelte` 锁定 `~1.0.1`（minor 锁定，避免 1.0 早期版本节点稳定性回漂）。
- `web/package.json`：版本切到 `0.2.1`。

### Version & Documentation

- Workspace 版本切到 `0.2.1`（9 crate 全量同步：gate-core / gate-storage / gate-crypto / gate-auth / gate-cache / gate-providers / gate-server / gate-billing / kgctl）。
- README badge 同步：`version-0.2.1`、`tests-285 Rust + 87 web`。

---

### Added — Documentation Architecture

- 新增 `CONTRIBUTING.md` 与 `SECURITY.md`，并把 `docs/architecture.md` 拆出 `data-plane` / `control-plane` / `worker-plane` 子页，形成更接近成熟项目的文档树与贡献入口。

- 新增 `docs/architecture.md` 作为长期系统架构入口，按 C4 system/context、runtime mode、route boundary、gateway request flow、data boundaries、deployment shapes 与 architecture decision log 组织，避免架构图散落在 `DESIGN.md`。
- `README.md`、`DESIGN.md` 与 `docs/README.md` 同步改为“README → architecture → DESIGN → runbook/stages”的阅读路径，关键文档与阶段性文档边界更清晰。

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
- Manifest Builder / Debugger 补齐：Channel 创建抽屉新增 7 步 builder（preset/auth/request/response sample/SSE replay/test/save+group），response sample 可点选生成 path mapping，Probe 步可填写初始 secret slot key，保存后自动写入 channel key、发起 manifest probe、同步发现模型并加入 channel group。
- `kgctl plugin test|export|import` 落地；`export` 生成包含 manifest、response sample、raw SSE 与 expected chunks 的 golden fixture，`import --verify` 可在 schema / normalizer 升级后回放验证。
- Provider capability matrix 落地：编译期 Provider 与 runtime plugin preset 共享 `ProviderCapabilities`（chat / streaming / tools / embeddings / image / audio / vision / json_mode / batch），Admin Channel / Group binding API 返回 capability，chat route 会按 stream/tools/vision/JSON mode 跳过不满足能力的 channel。
- Provider preset 增加 capability 默认值与 Base URL 建议；OpenAI-compatible 变体补齐 `vllm`、`lm_studio`、`ollama_openai`、`localai`、`xinference`，并新增 `vertex_openai` 作为 Google Vertex AI OpenAI-compatible 模板。
- HTTP Plugin embedding runtime 落地：`request.embedding_path` / `embedding_body` 与 `embedding_response` 支持 OpenAI-compatible 和私有 vector mapper；`/v1/embeddings` 现在可选择 `provider_type=plugin|custom|http|http_plugin` 且 `embeddings=true` 的 channel，并复用 active secret slot gating。
- Manifest registry 落地：新增 `examples/manifest-registry/registry.json` 官方/社区索引，记录 preset/sample 的 id、version、author、sha256、signature 与兼容范围；`kgctl plugin registry list|package|import|export` 支持官方/社区/私有 manifest 包导入导出，private entries 默认不导出。
- Manifest package 目录规范落地：新增 `examples/manifest-packages/private-auth-field-map-sse/` 样本，固定 `manifest.json`、`fixtures/` 请求/响应/SSE 样本、`README.md` 与 `security.md`，并增加 `kgctl plugin package lint --verify` 校验/回放。
- Plugin sandbox 安全边界产品化：`security.outbound_allowlist` 运行时强制 origin allowlist，绝对 URL 与 OAuth token URL 继续拒绝 localhost/private/link-local/metadata host，reqwest DNS resolver 与 response peer 双重阻断 DNS rebinding；`header_redaction` 合并默认敏感头并提供 redacted probe/debug request，query secret 在网络错误中脱敏；`request.timeout_ms` 覆盖 channel timeout，request/response/SSE limit、retry/cooldown/circuit breaker 与 manifest permissions 一并进入 schema/test/doc 闭环。
- WASM Plugin ABI vNext 设计稿落地：新增 `docs/wasm-plugin-abi.md`，明确 request/response/streaming transform、secret access API、deterministic execution constraints、resource limits、audit/metrics/trace 与 package/registry 边界；runtime 仍保持 vNext，不在 v0.2.0 暴露。
- P1.9 Prometheus metrics 命名收口：新增 `gateway_requests_total`、`gateway_request_duration_seconds`、`gateway_upstream_errors_total{kind,provider_type,channel,model}`、`quota_denies_total`、`billing_settle_lag_seconds`，并补齐 billing outbox enqueue/lag 指标与 `/metrics` smoke 覆盖。
- P1.9 Trace 串联收口：新增 `http.request`、`gateway.data_plane`、`gateway.upstream_request`、`billing.emit_usage`、`billing.outbox.*` 与 `billing.consumer.*` spans，用 `kooix.request_id` 串起 data-plane upstream、pricing/outbox 与 settlement。
- P1.9 控制台事故页落地：新增 `GET /v1/admin/incidents` 与 `/admin/incidents`，聚合最近错误、top failing channels、quota deny top、upstream 401/429/5xx 分类，并暴露 runtime-local quota / upstream error 快照辅助止血。
- P1.9 Runbook 收口：`docs/observability-runbook.md` 增加上游全挂、Redis 不可用、Postgres 慢查询、pricing sync 失败与 outbox backlog 的 signals / 止血 / 诊断 / 恢复链路。
- P2.1 前端模板一致性审计落地：新增 `scripts/audit-page-templates.mjs`，覆盖 25 个 Svelte route 页面的 `PageShell` / `AuthFrame`、`DataToolbar` / `FilterPanel` / `DataTable` 与 loading / error / empty 状态缺口清单。
- P2.1 表格能力基座落地：新增 `web/src/lib/table-state.ts` 管理 page size、sort、column visibility 与 saved filters；`/admin/audit` 迁移到 `PageShell` / `DataToolbar` / `DataTable`，并给 `/v1/admin/audit-logs` 补齐 `sort_by` / `sort_dir` 服务端排序。
- P2.1 `/admin/users` 推广表格模板与状态基座：用户列表和 session 面板改用 `DataToolbar` / `DataTable` / `ModalFrame`，支持 page size、列显隐与搜索/状态筛选持久化，模板审计缺口降至 11 页。
- P2.1 `/admin/incidents` 推广表格模板：Top failing channels 从 native table 迁到 `DataTable`，事故中心模板审计缺口清零，整体缺口降至 10 页。
- P2.1 `/orgs/[orgId]/quotas` 推广数据页模板：新增 `DataToolbar` 搜索/scope/mode 筛选与 active badges，分组 quota 表格改用 `DataTable`，模板审计缺口降至 9 页。
- P2.1 `/orgs/[orgId]/billing` 推广数据页模板：月账单页改用 `PageShell` / `DataToolbar` / `DataTable` / `StatePanel`，保留 CSV/JSON digest 导出、invoice 状态机与 quota alerts，模板审计缺口降至 8 页。
- P2.1 `/orgs/[orgId]/projects` 推广数据页模板：Project 列表页改用 `PageShell` / `DataTable` / `StatePanel`，保留 Org invite、创建项目、账单/配额跳转与项目设置/API Keys 操作，模板审计缺口降至 7 页。
- P2.1 `/orgs/[orgId]/projects/[projectId]/keys` 推广数据页模板：API Key 管理页改用 `PageShell` / `DataTable` / `ModalFrame` / `StatePanel`，保留明文 key 一次性展示、复制、撤销确认与刷新动作，模板审计缺口降至 6 页。
- P2.1 `/orgs/[orgId]/projects/[projectId]` 推广页面模板：Project 设置页改用 `PageShell` / `StatePanel`，保留 Project invite、项目设置、API Key quick create 与模型别名操作，模板审计缺口降至 5 页。
- P2.1 `/admin/sso` 推广数据页工具栏：SSO Provider 搜索从手写 search card 迁到 `DataToolbar`，新增清除搜索与 provider/enabled badges，模板审计缺口降至 4 页。
- P2.1 `/usage` 推广页面模板：用量仪表盘改用 `PageShell` / `DataToolbar` / `StatePanel`，保留 range / group_by / chart mode 切换、stat cards、趋势折线与模型/渠道柱状图，模板审计缺口降至 3 页。
- P2.1 `/setup` 推广认证页模板：首次初始化页改用 `AuthFrame` 与共享 theme toggle，保留两步 bootstrap、默认 Org/Project、完成后自动登录链路，模板审计缺口降至 2 页。
- P2.1 `/admin/groups` 推广页面与表格模板：渠道分组页改用 `PageShell` / `StatePanel`，Canary 对比与绑定列表迁到 `DataTable`，保留 fallback chain、inline binding 编辑与 modal 流程，模板审计缺口降至 1 页。
- P2.1 `/admin/channels` 推广仪表盘模板：渠道仪表盘改用 `PageShell` / `StatePanel`，最近错误 TOP 5 迁到 `DataTable`，保留导入/导出、Provider 健康分布与 quick links，模板审计缺口清零。
- P2.1 Pricing wizard 落地：`/admin/pricing` 新增 4 步向导（Model / Channel、dimension / unit / rate、价格预览、usage cost 模拟），前端 `pricing-preview` helper 镜像 `gate-billing::compute_cost` 的 token、image condition、batch / region multiplier 语义并补单测。
- P2.1 Quota wizard 落地：`/orgs/[orgId]/quotas` 新增 4 步向导（Scope、Model filter、RPM/TPM/Budget、Explain preview），一次生成多条 quota policy，并用后端 `explainQuota` 只读预览 would-deny。
- P2.1 UI 文案统一：控制台高频页收敛为中文主文案，保留 Provider / Channel / API Key / SSO / OIDC / Redis / PG 等术语，并新增 `ui-copy.test.ts` 防止 wizard、telemetry 与状态标签回漂。
- P2.2 ProviderRouter 增加 channel key 解密短缓存：`KOOIX_CHANNEL_KEY_CACHE_TTL_SECS` 默认 30s，控制面 create / rotate / revoke 与运行时 key failure 上报会显式失效对应 channel，外部 DB 直改最迟 TTL 后生效；`routing` Criterion bench 增加 key decrypt cache hit / disabled 对比。
- P2.2 hot path benchmark 补齐：`gate-server` 新增 `hot_paths` Criterion bench，覆盖 quota middleware 的 no-quota / rpm / body-metered budget 路径，以及 billing/request-log outbox enqueue 路径。
- P2.2 Usage/outbox 批量写入落地：`OutboxRepo` 增加 `enqueue_batch` / `mark_done_batch`，billing consumer 会批量写 `request_events`、`usage_records`、hourly/daily rollups 与 `billing_ledger_events.actual_settle`，duplicate `idempotency_key` 只结算一次但重复 outbox row 会被标记完成。
- P2.2 Request log 分区 / retention 落地：新增 `request_log_events` 月分区 read projection、`request_events` insert trigger、分区预建 helper 与 dry-run/apply retention helper；request log list/filter/incidents 优先读分区投影，`request_events` 继续保留幂等结算源语义。
- P2.2 SSE parser 压测补齐：`gate-providers` 新增 `sse` Criterion bench，覆盖小帧多、大帧、分片 UTF-8 与长连接取消；共享 `SseLineDecoder` 增加对应单测，避免流式 parser 边界回漂。
- P2.2 Web bundle 预算收口：Playground route 只保留轻量 shell 并动态加载 `FlowEditor`，MarkdownRenderer 仅在客户端按需动态加载 `marked` 与 `highlight.js` 语言包，`web/scripts/check-bundle-budget.mjs` 现在同时验证 route-level splitting、Flow editor lazy load 与 markdown highlighter lazy load。
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

- Quota policy engine 补全 P1.6：新增 `concurrent`、`lifetime_budget_usd`、`lifetime_tokens`，并支持 `mode=enforce|dry_run`。dry-run 只记录 `quota_dry_run_total` 与 would-deny tracing，不扣 Redis、不拦截请求。
- Quota middleware 按 `model_filter` 精确 / 简单 glob 过滤规则，TPM sliding window 支持按 estimated tokens 多单位记账，lifetime tokens settle 使用真实 usage tokens 而非 cost micros。
- 控制面新增 `/v1/orgs/:org_id/quotas/explain` 与 `/reconcile`：前者返回命中规则、当前消耗、剩余额度和恢复时间，后者对比 Redis counter 与 PG `usage_records` projection。
- Quota 控制台升级为 scope/model policy UI，支持 user / api_key / project / org、enforce / dry-run、lifetime budget、explain 预览与 Redis/PG 对账结果。
- 修复 budget quota pre-debit 的 `inflight_requests` 写入竞态：中间件不再后台 spawn insert，避免 handler 先 settle/delete 后 insert 才落库，导致同一 `x-request-id` 的 inflight 行残留并破坏 crash recovery 对账。
- budget quota pre-debit 支持解析 `EmbeddingRequest`，按 embedding input 字符数估算预扣，并在 `/v1/embeddings` 完成后用实际 `usage.total_tokens` settle / refund。
- `/v1/embeddings` 上游失败不再包装成 internal error；auth、rate limit、invalid request、policy、network、decode 与 mapped error 进入统一 provider error shape，并同步 channel key cooldown / circuit breaker 统计。
- budget quota pre-debit 支持解析 `ImageGenerationRequest`，按默认 `$0.08/image` 估算 image 请求预扣，并在 `/v1/images/generations` 完成后按 billable image units settle。
- `/v1/images/generations` 上游失败不再包装成 internal error，统一进入 provider error shape 与 channel key failure 统计。
- budget quota pre-debit 支持解析 `AudioSpeechRequest`，按 TTS input 字符数估算预扣，并在 `/v1/audio/speech` 完成后按 `tts_characters` settle。
- `/v1/audio/speech` 与 `/v1/audio/transcriptions` 上游失败不再包装成 internal error，统一进入 provider error shape 与 channel key failure 统计。

### Added — Identity / Sessions

- Refresh token 正式接入 `user_sessions`：登录 / SSO 会创建 session，只存 refresh token SHA-256 hash；refresh 时校验 session 未撤销/未过期并原子轮转 hash，旧 refresh token 重放返回 `token_invalid`。
- `/v1/auth/logout` 改为撤销当前 session，阻断后续 refresh；已签发 access token 仍按短 TTL 自然过期。
- 平台管理员新增用户 session 管理 API：`GET /v1/admin/users/:id/sessions`、`DELETE /v1/admin/users/:id/sessions/:session_id`、`DELETE /v1/admin/users/:id/sessions`，并写入 `user_session.revoke` / `user_session.revoke_all` audit。
- 控制台 `/admin/users` 增加 Session 面板，可查看 IP / User-Agent / last_used / expires_at，并执行单个撤销或全部踢下线；前端 refresh 流程会保存服务端返回的新 refresh token。
- `JwtRing` 支持 `KOOIX_JWT_SECRET` primary 签发 + `KOOIX_JWT_PREVIOUS_SECRETS` 旧 key 验签窗口，覆盖 access / refresh token 的正常 JWT secret rotation。
- `kgctl doctor` 新增 `KOOIX_JWT_PREVIOUS_SECRETS` 可选检查：逗号分隔 base64，每项至少 32B；`--json` 会报告窗口是否配置。
- SSO Provider 管理落地：新增 `/v1/admin/identity-providers` CRUD、`/discover` OIDC discovery、公开 `/v1/auth/sso/providers`，控制台 `/admin/sso` 支持 allowlist、auto-join role、enabled 状态与 redirect policy，登录页自动展示 enabled Provider。
- SSO `redirect_to` 增加 Provider 级 redirect policy：相对路径由 `allow_relative` 控制，绝对 URL 必须命中 `allowed_origins`；scheme-relative URL、`javascript:` 与未授权 origin 会在 start/callback 阶段拒绝。
- 邀请流落地：新增 org/project invitation create/list/revoke 与公开 preview/accept API，邀请 token 只存 SHA-256 hash，控制台在 Org / Project 页面可创建、复制、查看状态并撤销邀请，过期或已撤销邀请无法接受。
- SCIM 2.0 评估完成：新增 `docs/scim-evaluation.md`，明确用户同步字段、deprovision 策略、Org-scoped group → role mapping、安全边界与 vNext migration / API / UI 差距；当前不声明已提供 SCIM runtime endpoints。

### Added — P2.3 Security Hardening

- 新增 `docs/threat-model.md`，覆盖 tenant isolation、API key leakage、malicious plugin manifest、SSRF、billing fraud 与 admin account takeover。
- Admin 高危操作增加 `X-Kooix-Confirm` 二次确认：delete channel、rotate/revoke key、suspend user、change pricing、disable group。
- Audit log 扩展 actor subject、request_id、IP、User-Agent、project_id、before/after diff 与 error_message，并在控制台详情页展示。
- Secret redaction 接入 audit before/after 与 upstream error message，覆盖 password、secret、token、cookie、Authorization、Bearer、`sk-*` 与 query secret。
- `kgctl key rotate-master` 支持 dry-run、apply re-encrypt、verify 与 rollback plan，覆盖 `channel_keys.key_enc` 与 `identity_providers.client_secret_enc`。

### Added — P2.5 Release Assets

- 新增 `examples/demo/quickstart.sh`：`docker compose up`、首次 setup / admin 登录、创建 Provider preset channel、定价规则、Project API key、chat、usage / billing 一条链。
- 新增 `scripts/render-release-notes.mjs` 并接入 `.github/workflows/release.yml`，自动生成 changelog、Docker image tag、migration notes、known limitations 与 post-smoke。
- `RELEASE.md` 固化全门禁、gitleaks 双扫、demo script、GitHub Release notes 与截图/短视频 checklist。
- 新增 `docs/release-assets.md`，定义 Dashboard、Channel wizard、Pricing rules、Request logs、Playground 截图与 60-90 秒短视频脚本。

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
- Provider 插件预设落地：`model_mapping.plugin.preset.provider` 支持 `openai`、`openai_compatible`、`anthropic_messages`、`azure_openai`、`vertex_openai`、`gemini`、`deepseek`、`mistral`、`cohere_chat`、`ollama`、`groq`、`together`、`openrouter`、`moonshot`、`zhipu`、`qwen`、`yi`、`bedrock_converse` 等。
- 预设会补齐默认 path / headers / request adapter / response mapper / SSE mapper；OpenAI-compatible 自动注入 `stream_options.include_usage=true`，Azure 支持 deployment path 模板，Vertex AI 使用 Google OpenAI-compatible `/endpoints/openapi` 入口，Anthropic Messages / Bedrock Converse 具备基础 request adapter。
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
