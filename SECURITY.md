# Security Policy

## Supported security updates

Kooix Gate 的安全修复优先覆盖当前主分支和最新正式发布版本。

## Reporting a vulnerability

请通过私下渠道报告，不要直接公开 issue。

建议内容：

- 受影响的版本或提交
- 复现步骤
- 影响范围
- 相关请求、日志或截图
- 是否涉及 secret、tenant isolation、billing、admin mutation 或 plugin SSRF

## 重点关注面

- `docs/security-runbook.md`
- `docs/threat-model.md`
- `docs/plugin-manifest.md`
- `docs/observability-runbook.md`

## 高风险类别

- Secret 泄露
- Tenant 越权
- Plugin SSRF / 内网探测
- Billing / quota 绕过
- Admin takeover
- JWT / session 固化

## 处理原则

- 先止血，再复现，再修复。
- 所有 secret 统一脱敏。
- 需要修复时优先给出最小可逆补丁。
