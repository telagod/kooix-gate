# Contributing to Kooix Gate

欢迎提交 PR。

## 1. 先看什么

提交前先读：

- `README.md`
- `docs/architecture.md`
- `DESIGN.md`
- `docs/README.md`
- `RELEASE.md`
- `docs/security-runbook.md`

## 2. 贡献前检查

```bash
git status --short
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
cd web && npm run check && npm test && npm run build
```

涉及路由边界或文档结构时，再补：

```bash
node scripts/check-route-manifest.mjs
node scripts/generate-route-types.mjs --check
```

## 3. 提交原则

- 小步提交，单次只解决一类问题。
- 文档改动要同步索引和入口。
- 运行时边界改动必须更新 `docs/architecture.md` 或 `DESIGN.md`。
- 安全 / 权限 / 密钥相关改动必须更新 `docs/security-runbook.md` 或 `docs/threat-model.md`。

## 4. PR 要求

PR 描述至少包含：

- 改了什么
- 为什么改
- 怎么验证
- 是否影响发布 / 数据 / 安全边界

## 5. 文档规范

- 关键文档放根目录或 `docs/README.md` 索引中的长期入口。
- 已完成的一次性审计、验证、收口材料放 `docs/stages/`。
- 不要把完成态说明散落到多个 README。
