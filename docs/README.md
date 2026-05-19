# Kooix Gate 文档索引

本目录只放两类文档：长期有效的关键文档，以及已经完成的阶段性记录。根目录仍保留项目入口、设计、路线、变更和发布文档。

## 关键文档

| 文档 | 用途 |
| --- | --- |
| [README.md](../README.md) | 项目入口、能力速览、Quick Start、测试命令。 |
| [DESIGN.md](../DESIGN.md) | 架构设计、核心决策、运行时边界。 |
| [ROADMAP.md](../ROADMAP.md) | 当前产品路线与未完成阶段计划。 |
| [CHANGELOG.md](../CHANGELOG.md) | 版本变更记录。 |
| [RELEASE.md](../RELEASE.md) | 发布、回滚、部署前后 smoke runbook。 |
| [AGENTS.md](../AGENTS.md) | 仓库级工程规则与 Codex 执行约束。 |
| [CLAUDE.md](../CLAUDE.md) | 与 AGENTS 同步的项目规则副本。 |
| [plugin-manifest.md](./plugin-manifest.md) | HTTP Plugin manifest v0 边界、示例与安全约束。 |
| [security-runbook.md](./security-runbook.md) | 密钥、JWT、Channel key、Redis quota、Plugin 风险处置。 |
| [observability-runbook.md](./observability-runbook.md) | Gateway、billing、worker 指标与 PromQL 入口。 |

## 模块文档

| 文档 | 用途 |
| --- | --- |
| [web/README.md](../web/README.md) | SvelteKit 控制台开发、构建、页面与 API ID 约定。 |
| [web/src/lib/design/README.md](../web/src/lib/design/README.md) | 前端设计模板、token、组件分层与美学约束。 |
| [crates/kgctl/README.md](../crates/kgctl/README.md) | `kgctl` 部署 / 运维 CLI 使用说明。 |
| [bench/README.md](../bench/README.md) | 50k rpm 负载测试与 mock upstream 说明。 |
| [examples/README.md](../examples/README.md) | SDK、curl、Postman、Bruno、OpenAPI、Terraform、Helm 示例入口。 |

## 阶段性文档

阶段性文档只记录某一轮审计、迁移、收口或验证证据；完成后不再作为主入口阅读。

- [stages/](./stages/)：已完成阶段的审计与收口记录。

## Waivers

Waiver 是仍在生效的质量 / 安全例外，不归档到阶段性目录；路径可能被脚本或 CI 引用。

| 文档 | 状态 |
| --- | --- |
| [waivers/quality/2026-05-19-large-files.md](./waivers/quality/2026-05-19-large-files.md) | active；`scripts/quality-gate.mjs` 使用。 |
| [waivers/security/2026-05-19-rsa-marvin-openidconnect.md](./waivers/security/2026-05-19-rsa-marvin-openidconnect.md) | active；对应 `cargo audit --ignore RUSTSEC-2023-0071`。 |
