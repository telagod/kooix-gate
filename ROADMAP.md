# Kooix Gate Roadmap

> **战略主线**：批量号池管理 + 难接入渠道 + Rust 性能预算撑起的合规过滤层。
> 不是又一个 API 聚合，不是又一个 LLM 路由 SDK。

## 三里程碑

| 里程碑 | 主题 | 时间盒 | 破坏性 |
|--------|------|--------|--------|
| **M5 · v0.5.0** 渠道打磨 + 健康度自愈 | 号池中台的分水岭：`ChannelHealthScore` + 自动 cooldown + 封号检测 + 故障自动转移 | 4-6 周 | 否 |
| **M6 · v0.6.0** 合规过滤模块库 | 把 Rust 性能预算变现：官方 PII redact / moderation / prompt-injection 三个 WASM ref 模块 + 流式 chunk-level 审核 | 6-8 周 | 否（含 v0.5.0 deprecation） |
| **M7 · v0.7.0** 难接入渠道标杆 | Native provider plane 扩面：Codex / Kiro / Windsurf 之外补 3-5 个"逆向才能接"的渠道，写成 deep-dive 标杆 | 6-8 周 | 取决于上游协议漂移 |

> 历史里程碑 M1/M2/M3/M4（v0.2.x → v0.4.x）的详细路线与已完成基线归档：[docs/archive/roadmap/ROADMAP-pre-0.5.0.md](./docs/archive/roadmap/ROADMAP-pre-0.5.0.md)。

---

## M5 · v0.5.0 — 渠道打磨与健康度自愈

**主题**：号池中台 vs 普通 API 聚合的分水岭，是「账号生命周期管理」。
当前路由策略只看 `inflight / latency`，看不到「这号要废了」。M5 把渠道从「能用」打到「能自愈」。

### M5.1 ChannelHealthScore（核心）

- [ ] **N1.1** 健康度评分模型：`success_rate / latency_p99 / banned_signal / quota_remaining / consecutive_5xx`，输出归一 0-1 分。
- [ ] **N1.2** 状态机：`Healthy → Degraded → Cooldown → Banned → Recovering`，转移规则文档化。
- [ ] **N1.3** 路由策略消费 score：`priority / weighted_random / least_conn / least_latency` 全部接受 health 权重。
- [ ] **N1.4** 自动 cooldown：检测到 401/403/429 模式后指数退避，可配置最大 cooldown 时长。
- [ ] **N1.5** 封号检测器：基于响应特征（特定 error code / response body / 余额耗尽信号）触发状态转移。
- [ ] **N1.6** 控制台「号池健康仪表盘」：channel × score × state × cooldown_until × banned_reason。

### M5.2 账号画像与运营

- [ ] **N2.1** Channel metadata 扩字段：注册时间 / 付费类型 / 区域 / 上次封号原因。
- [ ] **N2.2** 余额监控：可声明 balance probe path + 阈值告警。
- [ ] **N2.3** 充值告警 webhook + Prometheus metric。
- [ ] **N2.4** 号池「批量准入」：CSV 导入 N 个账号，自动 probe + 健康度初始化。

### M5.3 ADR-0007 落地

- [ ] **N3.1** ADR-0007 `ChannelHealthScore` Accepted（设计稿先于实装）。
- [ ] **N3.2** `health_check.rs` 1019 行 god file 按 health-score 边界重写（与 N1.1/N1.2 合并）。

### M5.4 v0.5.0 砍单 Wave 1-3（瘦身闭环）

详见 [docs/cut-list-v0.5.0.md](./docs/cut-list-v0.5.0.md)（待出）：

- [x] **K13** ROADMAP 重写为号池叙事 — 本文件
- [x] **K4** product-review 三刀归档
- [x] **K5** backlog 设计稿挪位
- [x] **K1** Playground 整段砍（认知瘦身王牌 + 340KB bundle）
- [ ] **K15** ADR 状态重排（ADR-0002/0003/0004 标 Superseded）
- [ ] **K14** CHANGELOG 0.4.x 流水账折叠
- [ ] **K8** `api.ts` 按域拆分（先抽 `core.ts` 共享 apiFetch/ApiError）
- [ ] **K2** Provider preset 55 → 12（保留 4 个 fast-path + 8 个核心；存量 channel 走只读降级）
- [ ] **K6** examples 收敛（先迁 WASM fixture）
- [ ] **K7** 模板瘦身（按 ripgrep 复用统计逐个 inline）

### M5.5 v0.5.0 验收门禁

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build
```

发版条件：

- ChannelHealthScore 在生产路径生效（路由策略消费 score）。
- 任一 channel 出现 401/403/封号特征 → 自动 cooldown + 状态转移。
- 控制台「号池健康仪表盘」可定位每条 channel 的实时状态与封号原因。
- 测试 `#[ignore]` 列表清零（ADR-0004 plugin harness 补全）。

---

## M6 · v0.6.0 — 合规过滤模块库

**主题**：把「Rust 性能预算 = 路由 + 计费」的浪费补上。
合规过滤是号池中台 vs API 聚合的第二条护城河。当前 WASM ABI 是空管道，没官方 ref 实现，魔尊说的「性能优势给隐私过滤/内容审核留空间」未兑现。

### M6.1 三个官方 ref 模块

- [ ] **`pii-redact-v1`**：正则 + 词典 + 上下文白名单；身份证 / 手机号 / 邮箱 / API key 模式 / 自定义词典。
- [ ] **`moderation-v1`**：本地关键词命中 + 可选 OpenAI moderation API 旁路；命中后动作可声明（拦截 / 脱敏 / 打标 / 降级）。
- [ ] **`prompt-injection-v1`**：启发式 + 模式匹配（"忘记之前指令"、伪 system 标签、角色覆写词），输出风险分。

每个模块都自带：`fixture/` 触发样本 + `golden/` 预期输出 + 性能 bench（p99 latency 增量必须 < 3ms）。

### M6.2 流式 chunk-level 审核

- [ ] **N6.2.1** WASM ABI v1 增 `transform_stream_chunk` hook（已设计，未实装）。
- [ ] **N6.2.2** Chunk 命中违规后的动作语义：截流 / 替换 / 注入警告 / 静默打标。
- [ ] **N6.2.3** SSE event-by-event transform e2e。

### M6.3 预算式过滤

- [ ] **N6.3.1** Per-request budget：例如「允许花 3ms 做 PII 扫描，超时降级到正则」。
- [ ] **N6.3.2** 过滤模块的资源限制：内存 / CPU time / fuel。
- [ ] **N6.3.3** 性能 bench 入 README：纯路由 vs 路由 + PII + moderation 的 p99 延迟差。

### M6.4 配合的渐进退役（deprecation window）

- [ ] **K3** WasmtimeHost v0 物理删除（前置：所有 v0 引用迁 ComponentHost）。
- [ ] **K10** billing invoice 状态机简化（DROP TABLE billing_invoices 走 down migration + 2 大版本 deprecation）。

---

## M7 · v0.7.0 — 难接入渠道标杆

**主题**：「难接入渠道」是 Kooix Gate vs OneAPI/NewAPI/LiteLLM 的第三条护城河。
当前已有 native provider plane（codex/kiro/windsurf），但是单点，没成体系。M7 把这条线打造成引流锚点。

### M7.1 Session / Cookie 生命周期

- [ ] **N7.1.1** Cookie/Session 持久化与刷新：登录态渠道（需要 session cookie 的服务）的统一存储 + 自动续期。
- [ ] **N7.1.2** Session 死亡检测 + 自动重登流程钩子（用户提供 refresh callback）。
- [ ] **N7.1.3** Session 加密落库（envelope encryption，复用 channel_key crypto）。

### M7.2 反指纹（高端难接入必备）

- [ ] **N7.2.1** UA / Accept-Language / TLS fingerprint 可声明轮换（按 channel 配置）。
- [ ] **N7.2.2** Manifest 增 `client_profile` 字段：声明客户端身份特征。
- [ ] **N7.2.3** TLS JA3/JA4 fingerprint 控制（reqwest + rustls 适配评估）。

### M7.3 请求录制与回放

- [ ] **N7.3.1** 线上号池 session 录制（区别于现有 `custom_provider/replay.rs` 的 fixture replay）。
- [ ] **N7.3.2** 录制脱敏 + 安全存储（敏感字段自动 redact）。
- [ ] **N7.3.3** Debug UI：跳过私有协议黑盒，看真实请求/响应/SSE 帧。

### M7.4 标杆 case study

- [ ] **N7.4.1** Deep-dive 博客：Codex / Kiro / Windsurf "为什么难接入、Kooix 怎么解决"。
- [ ] **N7.4.2** 补 3-5 个新 native provider（候选：需要登录态的私有服务 / 地区限制的 LLM / 逆向才能接的 web API）。
- [ ] **N7.4.3** Native provider plane 接入文档（让外部贡献者也能加）。

---

## 战略主线（不变）

Kooix Gate **不是** 又一个 OpenAI-compatible proxy。真正护城河三条：

1. **号池中台**：账号生命周期管理（健康度 / 封号自愈 / 余额监控 / 账号画像）。OneAPI 没有，LiteLLM 没有。
2. **合规过滤层**：Rust 性能预算撑起的 PII / moderation / prompt-injection 流式审核。Python 栈跑不动 chunk-level。
3. **难接入渠道**：native provider plane + session / 反指纹 / 录制回放。SaaS 玩家不愿做，OSS 玩家做不来。

### 不做

- ❌ LangFlow / Dify 风格的工作流可视化编辑器（v0.5.0 砍 Playground）。
- ❌ 100+ provider preset 数量竞赛（v0.5.0 收敛到 12 个，长尾走 community/ PR）。
- ❌ 企业 SSO / SAML / SCIM 主线投入（M5/M6/M7 不进，等社区需求驱动）。
- ❌ Stripe / 支付 gateway 集成（号池场景普遍预付费，不需要月结状态机）。

---

## 当前基线（v0.5.0-rc2 · 2026-06-18）

> v0.4.x 已交付完整网关底盘 + WASM ABI + 4 刀 product-review 收口。详细历史见 [archive/ROADMAP-pre-0.5.0.md](./docs/archive/roadmap/ROADMAP-pre-0.5.0.md)。

- 多 Org / Project / ApiKey 三层租户 + RBAC + RLS 双闸隔离。
- 4 个 fast-path provider（OpenAI / Anthropic / Azure / Bedrock）+ HTTP Plugin manifest v1（55 preset，**v0.5.0 收敛到 12**）+ Native provider plane（Codex / Kiro / Windsurf）。
- WASM ABI v1（Component Model）+ Rust SDK + AssemblyScript SDK + host functions（log / record_metric / get_secret_slot）。
- Channel group 路由：priority / weighted_random / round_robin / least_conn / least_latency + fallback + canary。
- 计费：outbox + fail-closed pre-debit + ledger + invoice state machine（**v0.6.0 简化**）。
- 配额：rpm / tpm / concurrent / daily / monthly / lifetime / budget + dry-run + explain。
- typed ID API response + `FlexUuid` path 兼容。
- SvelteKit 控制台：Channel / Group / Pricing / Quota / Usage / Requests / Billing / SSO / Users / Incidents / Audit（**v0.5.0 砍 Playground**）。
- CI：Rust fmt / clippy / check / nextest + Web check / vitest / build；556+ Rust tests + 127 web tests。

---

## 验收门禁（每个里程碑通用）

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
rg 'fn.*\(.*AuthContext' crates/gate-server/src -A 5 | rg -v 'require!|can!|require_user!|require_api_key!'
rg 'password|secret|token|sk-' . --glob '!target' --glob '!web/node_modules' --glob '!Cargo.lock'
```

---

## 历史里程碑（已收口）

| 里程碑 | 主题 | 版本 | 状态 |
|--------|------|------|------|
| M1 · v0.2.1 | 文档定位 + 三巨兽拆解 + 前端散乱收口 | v0.2.1 | ✅ |
| M2 · v0.3.0 | 编译期 Provider 退役（ADR-0001） | v0.3.0 | ✅ |
| M3 · v0.4.0 | Fast-path runtime（ADR-0002）+ WASM ABI v0（ADR-0003） | v0.4.0-v0.4.58 | ✅ |
| M4 · v0.5.0-rc | product-review 四刀 + 阶段小版收口 | v0.4.65-v0.4.181 | ✅ |

完整路线总览、P0/P1/P2 已完成基线证据、四刀 product-review 战报全部归档于：
[docs/archive/roadmap/ROADMAP-pre-0.5.0.md](./docs/archive/roadmap/ROADMAP-pre-0.5.0.md)（1082 行）。
