# Kooix Gate 文档索引

本目录只放两类文档：长期有效的关键文档，以及已经完成的阶段性记录。根目录仍保留项目入口、设计、路线、变更和发布文档，避免完成态材料散落在根目录。

- **关键文档**：当前开发、部署、排障和发布仍会反复引用的 source of truth。
- **阶段性文档**：某一轮审计、迁移、验证或发布验尸的完成态证据；读者需要背景时再进入，不作为当前入口。

建议阅读顺序：

1. `README.md`：先看项目定位与快速开始。
2. `docs/getting-started.md`：三档接入路径与首个 chat。
3. `docs/architecture.md`：再看系统架构总览与子页面导航。
4. `docs/architecture/data-plane.md`：确认热路径边界。
5. `docs/architecture/control-plane.md`：确认管理面边界。
6. `docs/architecture/worker-plane.md`：确认后台任务边界。
7. `DESIGN.md`：确认领域模型、权限、配额、计费等长期设计原则。
8. `ROADMAP.md`：确认当前路线和未完成阶段。
9. `docs/product-gaps.md`：v0.5.0 启动会议筛选清单，标注影响面与实施路径。
10. `RELEASE.md` / `docs/*runbook*`：看部署与处置。
11. `docs/stages/`：最后看已完成阶段证据。

## 关键文档

| 文档 | 用途 |
| --- | --- |
| [README.md](../README.md) | 项目入口、能力速览、Quick Start、测试命令。 |
| [getting-started.md](./getting-started.md) | 三档接入：30 秒 Docker / 5 分钟 Helm / 10 分钟本地源码 + WASM transform 接入示例。 |
| [architecture.md](./architecture.md) | 系统架构总览入口、C4、runtime mode、route boundary 与关键请求流。 |
| [architecture/data-plane.md](./architecture/data-plane.md) | Data Plane 关键边界与代码锚点。 |
| [architecture/control-plane.md](./architecture/control-plane.md) | Control Plane 管理面边界与代码锚点。 |
| [architecture/worker-plane.md](./architecture/worker-plane.md) | Worker Plane 后台任务边界与代码锚点。 |
| [DESIGN.md](../DESIGN.md) | 设计原则、领域模型、权限、配额、计费与演进边界。 |
| [ROADMAP.md](../ROADMAP.md) | 当前产品路线与未完成阶段计划。 |
| [product-gaps.md](./product-gaps.md) | v0.4.60 → v0.5.0 产品化缺口对账清单，0.5.0 启动会议筛选据此。 |
| [CHANGELOG.md](../CHANGELOG.md) | 版本变更记录。 |
| [RELEASE.md](../RELEASE.md) | 发布、回滚、部署前后 smoke runbook。 |
| [api-reference.md](./api-reference.md) | OpenAPI / Postman / Bruno + 关键 API 索引与统一 error shape。 |
| [observability.md](./observability.md) | Prometheus metric 命名表、Grafana dashboard、OTLP span 字段表。 |
| [AGENTS.md](../AGENTS.md) | 仓库级工程规则与 Codex 执行约束。 |
| [CLAUDE.md](../CLAUDE.md) | 与 AGENTS 同步的项目规则副本。 |
| [plugin-manifest.md](./plugin-manifest.md) | HTTP Plugin manifest v1 schema、兼容升级、示例与安全约束。 |
| [wasm-plugin-abi.md](./wasm-plugin-abi.md) | WASM Plugin ABI v0 设计（0.4.16 PoC → 0.4.60 实装）：transform / secret / determinism / 资源限制 / audit。 |
| [wasm-runbook.md](./wasm-runbook.md) | WASM transform 上线后的故障处置：load 失败 / timeout-OOM / panic 暴风雨 / 模块回滚。 |
| [wasm-sdk-as.md](./wasm-sdk-as.md) | AssemblyScript 写 WASM transform 的最小可用方案，与 Rust SDK 对照。 |
| [manifest-registry-signature.md](./manifest-registry-signature.md) | Registry 签名 schema（cosign / minisign / sigstore_bundle），typed `kind/value/key_id/alg` 字段。 |
| [scim-evaluation.md](./scim-evaluation.md) | P1.7 SCIM 2.0 用户同步与 group → role mapping 评估，定义 vNext 实现边界。 |
| [threat-model.md](./threat-model.md) | P2.3 威胁模型：tenant isolation、secret leakage、plugin SSRF、billing fraud、admin takeover。 |
| [release-assets.md](./release-assets.md) | P2.5 发布截图 / 短视频素材 checklist，覆盖 Dashboard、Channel wizard、Pricing、Request logs、Playground。 |
| [security-runbook.md](./security-runbook.md) | 密钥、JWT、Channel key、Redis quota、Plugin 风险处置。 |
| [observability-runbook.md](./observability-runbook.md) | Gateway、billing、worker 指标、PromQL 入口与 P1.9 事故 Runbook。 |
| [playground.md](./playground.md) | Visual Workflow Editor 产品线说明与节点能力联动。 |

## 模块文档

| 文档 | 用途 |
| --- | --- |
| [web/README.md](../web/README.md) | SvelteKit 控制台开发、构建、页面与 API ID 约定。 |
| [web/src/lib/design/README.md](../web/src/lib/design/README.md) | 前端设计模板、token、组件分层与美学约束。 |
| [crates/kgctl/README.md](../crates/kgctl/README.md) | `kgctl` 部署 / 运维 CLI 使用说明。 |
| [bench/README.md](../bench/README.md) | 50k rpm 负载测试与 mock upstream 说明。 |
| [examples/README.md](../examples/README.md) | SDK、curl、Postman、Bruno、OpenAPI、Terraform、Helm 示例入口。 |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 贡献前检查、提交原则、PR 要求与文档规范。 |
| [SECURITY.md](../SECURITY.md) | 安全披露、重点关注面与高风险类别。 |
| [examples/terraform/README.md](../examples/terraform/README.md) | Terraform provider / resource 示例入口。 |
| [examples/manifest-registry/registry.json](../examples/manifest-registry/registry.json) | 官方/社区 HTTP Plugin manifest registry 索引，可用 `kgctl plugin registry` 导入私有包。 |
| [examples/manifest-packages/private-auth-field-map-sse/](../examples/manifest-packages/private-auth-field-map-sse/) | HTTP Plugin manifest package 目录规范样本，覆盖 `manifest.json`、`fixtures/`、`README.md`、`security.md`。 |

## 阶段性文档

阶段性文档只记录某一轮审计、迁移、收口或验证证据；完成后不再作为主入口阅读。

- [stages/](./stages/)：已完成阶段的审计与收口记录。
- 新完成的审计 / 迁移 / 安全扫描 / 发布验尸记录统一归入 `docs/stages/YYYY-MM-DD-topic.md`，不要继续堆到根目录；同一连续阶段可追加到已有阶段文件，避免产生碎片文档。

## Waivers

Waiver 是仍在生效的质量 / 安全例外，不归档到阶段性目录；路径可能被脚本或 CI 引用。

| 文档 | 状态 |
| --- | --- |
| [waivers/quality/2026-05-19-large-files.md](./waivers/quality/2026-05-19-large-files.md) | active；`scripts/quality-gate.mjs` 使用。 |
| [waivers/security/2026-05-19-rsa-marvin-openidconnect.md](./waivers/security/2026-05-19-rsa-marvin-openidconnect.md) | active；对应 `cargo audit --ignore RUSTSEC-2023-0071`。 |
