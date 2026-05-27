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

## 6. Disk usage management

Kooix Gate 是 9 crate workspace，跨 crate integration test 多，`target/debug` 容易膨胀到 100GB+。请定期清理。

> ⚠ **2026-05-23 真实事故**：本仓库 `target/` 曾膨胀到 **240G**（debug/incremental 99G + debug/deps 137G），
> 参与触发一次系统 OOM 卡死。完整复盘与系统性预防策略见 [`docs/build-hygiene-runbook.md`](docs/build-hygiene-runbook.md)。

### cargo-sweep（推荐）

```bash
cargo install cargo-sweep
bash scripts/cargo-sweep-helper.sh           # dry-run，看预计释放空间
bash scripts/cargo-sweep-helper.sh --apply   # 真删
bash scripts/cargo-sweep-helper.sh --deep    # 深度（包含 cargo clean -p gate-storage）
```

默认清理 30 天前 fingerprint。可用 `KOOIX_SWEEP_DAYS=7` 覆盖阈值。

### cargo-nextest（推荐）

`cargo nextest run` 比 `cargo test` 快 30-50%，且能复用 binary。

```bash
cargo install cargo-nextest --locked
cargo nextest run --workspace                # 默认 profile
cargo nextest run --workspace --profile ci   # CI profile（fail-fast=false）
```

配置见 `.config/nextest.toml`。doctest 仍走 `cargo test --workspace --doc`（nextest 不支持 doctest）。

### 跨 crate integration test 分布

各 crate `tests/` 目录测试责任：

| crate | tests/ 数量 | 责任 |
|-------|------------|------|
| gate-storage | 5 | Repo trait + RLS + migration（sqlx-macro 耦合） |
| gate-providers | 2 | Provider trait + plugin runtime e2e |
| gate-cache | 1 | Redis Lua 脚本 |
| gate-billing | 2 | Pricing + outbox consumer 单元 |
| gate-server | 19 | 跨 crate 集成（auth/chat/billing/quota/sso/etc.） |
| kgctl | 1 | CLI 端到端 |

**分布原则**：每个 crate 测自己的 crate 边界；真正的跨 crate e2e 集中在 gate-server。
**编译产物优化**：不通过迁移 test 文件位置（耦合）解决，而是用 cargo-nextest 复用 binary。

### sqlx migrate cache

新增 migration 后跨 crate 测试要先：

```bash
cargo clean -p gate-storage
cargo test -p gate-server
```

否则 sqlx-macro 缓存会用旧 schema，跨 crate 测试会失败。

### dev profile

`Cargo.toml` 已设：

```toml
[profile.dev]
debug = "line-tables-only"        # 比 debug=1 小 3-4 倍
split-debuginfo = "unpacked"      # Linux 上显著减小
[profile.dev.package."*"]
opt-level = 1                      # 依赖 opt 1，本工程仍 0
```

如有特殊调试需求需要完整 debug info，临时 override 即可。

