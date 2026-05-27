# Security Policy

## Supported Versions

Kooix Gate 的安全修复优先覆盖以下版本：

| 版本 | 状态 | 安全更新窗口 |
|------|------|------------|
| 0.4.x（current）| ✅ Actively supported | 主线 main，所有补丁随 patch release 发布 |
| 0.3.x | ⚠ Security-only | 仅严重漏洞反向移植；不支持新功能 |
| ≤ 0.2.x | ❌ End-of-life | 不再接收任何修复，请升级 |

v0.5.0-rc1 发布后 0.3.x 即进入 EOL。

## Reporting a Vulnerability

**请通过私下渠道报告，不要直接公开 issue / PR / discussion**。公开报告会让漏洞在补丁可用前被利用。

### 联系方式（按推荐优先级）

1. **GitHub Security Advisory**（推荐）— [新建 advisory](https://github.com/telagod/kooix-gate/security/advisories/new)
   - 仅 maintainer 与你可见
   - 内置 CVE 申请流程
   - 修复后可一键发布 advisory
2. **Email** — security@kooix-gate.invalid（待启用）
   - 请用 PGP 加密敏感细节
   - 主题前缀 `[SECURITY]`
3. **紧急情况** — telagod 在 GitHub profile 列的联系方式

### 报告应包含

- **受影响版本或 commit hash**
- **完整复现步骤**（curl / Postman / minimal repro code）
- **影响范围评估**（攻击者前提条件、可控范围、潜在数据 / 资金损失）
- **相关请求、日志或截图**（请先脱敏 secret / PII）
- **建议修复方向**（可选，但欢迎）
- **是否已私下告知其他方**（如 cloud provider / 上游 SDK 作者）

## Response SLA

| 阶段 | 时间 | 内容 |
|------|------|------|
| **Initial acknowledgement** | ≤ 72 小时 | 确认收到 + 分配 tracking ID |
| **Triage** | ≤ 7 天 | 评估严重程度（CVSS 3.1）+ 决定是否需要 embargo |
| **Fix in private branch** | 严重 ≤ 14 天 / 中 ≤ 30 天 / 低 ≤ 90 天 | 在私有 fork 完成 + 测试 |
| **Coordinated disclosure** | Fix release + 7 天 | 发布 GitHub Security Advisory + CVE + 升级指南 |

SLA 仅承诺响应；具体修复时间因复杂度 / 上游依赖等可延长，但每延期一次主动通知报告者。

## Coordinated Disclosure

我们采用**协同披露**：

1. 报告者私下提交 → maintainer 修复 → embargo 期内不公开任何技术细节
2. 修复版本发布后 7 天 → 发布 advisory + CVE 编号 + 升级路径
3. 报告者可在 advisory 中署名（如不愿署名请明示）
4. 不接受勒索式威胁（"X 天内不修就公开"）—— 但若 maintainer 长期不响应（≥ 90 天），报告者有权公开

## 高风险类别（重点关注面）

按攻击表面优先级：

### P0（critical，72h 内响应）

- **Secret 泄露**：master_key / JWT secret / channel key / OIDC client_secret 在任何路径（log / audit / error response / metric label）出现
- **Tenant 越权**：A org 的请求拿到 B org 的资源 / API key / quota / billing
- **Admin takeover**：未授权用户拿到 PlatformAdmin 权限
- **Plugin SSRF / 内网探测**：HTTP plugin manifest 或 WASM plugin 访问 link-local / private IP / metadata endpoint

### P1（high，7d 内响应）

- **Billing / quota 绕过**：跳过预扣 / 双扣 / 跨 org 计费
- **JWT / session 固化**：JWT secret 泄露后无法及时回收 / refresh token 不能撤销
- **Provider upstream body 泄漏**：上游错误 response 含 PII / key 进入 audit 或 client response（0.4.69 已修，持续监控回归）

### P2（moderate）

- **Rate limit 绕过**：fail-open 路径被利用做大流量
- **WASM resource exhaustion**：恶意 plugin 通过 fuel / memory 耗尽影响其他 channel
- **Audit log 完整性**：审计事件丢失 / 篡改 / 投递保证不足

## 重点关注文档

- [`docs/security-runbook.md`](./docs/security-runbook.md) — 7 类安全事件运维步骤
- [`docs/threat-model.md`](./docs/threat-model.md) — STRIDE 威胁建模（7 个条目）
- [`docs/plugin-manifest.md`](./docs/plugin-manifest.md) — HTTP plugin manifest 安全约束
- [`docs/wasm-plugin-abi.md`](./docs/wasm-plugin-abi.md) — WASM plugin ABI / sandbox 边界
- [`docs/observability-runbook.md`](./docs/observability-runbook.md) — 异常检测信号

## 处理原则

- **止血优先**：先发布临时禁用 / 配置变更指南，再做根因修复
- **Secret 强制脱敏**：所有 log / audit / error response 走 `redact_upstream_body` + `audit_redaction` 链
- **最小可逆补丁**：避免大重构，让 backport 到 0.3.x 容易
- **覆盖测试**：每个修复必须配 regression test
- **公开后给升级指南**：advisory 必须包含 affected version + minimum safe version + workaround（如果有）

## Security Advisory 历史

参见 [GitHub Security Advisories](https://github.com/telagod/kooix-gate/security/advisories) 已发布的 advisory 列表。

截至 0.4.182（2026-05-28），尚无公开 advisory。

## 不在范围内（NOT a vulnerability）

以下情况**不算安全漏洞**，请走 [GitHub issues](https://github.com/telagod/kooix-gate/issues) 而不是 security advisory：

- 性能问题（除非可被利用造成 DoS）
- 文档错误 / 拼写错误
- 默认配置选择（如 fail-open 限流策略 — 这是 docs/security-runbook.md 已说明的设计决策）
- 错误信息不够友好
- 缺少功能（feature request）
- 依赖的第三方库漏洞 — 请直接报给上游，我们会跟踪并 bump dependency
