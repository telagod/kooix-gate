# Release Runbook

> v0.2.0 起，每次发布必须有版本、迁移、镜像、回滚与验尸记录。

## 版本策略

- 正式版本使用 SemVer tag：`vMAJOR.MINOR.PATCH`。
- 第一个正式版本：`v0.2.0`。
- Rust workspace version、README 当前版本、CHANGELOG 标题必须一致。
- Docker image tag：
  - `ghcr.io/telagod/kooix-gate:v0.2.0`
  - `ghcr.io/telagod/kooix-gate:latest`

## 发布前检查

```bash
git status --short
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && npm run check && npm test && npm run build
cd ..
git diff --check
```

涉及 migration 时额外跑：

```bash
cargo clean -p gate-storage
cargo test -p gate-storage --test pg_repo
cargo test -p gate-server --test auth_flow
```

部署环境发布前：

```bash
kgctl env
kgctl migrate --dry-run
kgctl migrate
kgctl doctor
```

`kgctl doctor` 会检查：

- `KOOIX_MASTER_KEY` base64 32B。
- `KOOIX_JWT_SECRET` base64 至少 32B。
- `KOOIX_JWT_PREVIOUS_SECRETS` 未配置或逗号分隔 base64 ≥32B（JWT rotation 旧 key 验签窗口）。
- `KOOIX_PUBLIC_URL` 是 http/https 根 URL。
- PostgreSQL 可达且 `_sqlx_migrations` 已到最新版本。
- Redis 可达，且 rate limit / quota Lua 脚本可执行。

## 数据库兼容边界

- v0.2.0 默认 migration 在普通 PostgreSQL 15+ 上运行，不要求 TimescaleDB。
- 高吞吐生产环境建议把 `usage_records` 升级为 TimescaleDB hypertable，按天分块、压缩与保留策略治理体量。
- 当前仓库未启用 `.sqlx` 离线 prepare；发布门禁以 `cargo check/test`、migration 测试与 `kgctl doctor` 校验。

## 打 tag 与推送

```bash
VERSION=v0.2.0

git switch main
git pull --ff-only origin main
git tag -a "$VERSION" -m "Kooix Gate $VERSION"
git push origin "$VERSION"
```

Tag push 会触发 `.github/workflows/docker.yml`，构建并推送 GHCR 镜像。

## GitHub Release

```bash
gh release create v0.2.0 \
  --title "Kooix Gate v0.2.0" \
  --notes-file /tmp/kooix-gate-v0.2.0-notes.md
```

Release notes 至少包含：

- 关键新增能力。
- Migration notes。
- Docker image tag。
- Known limitations。
- 回滚说明。

## 回滚策略

### 应用回滚

1. 切回上一镜像 tag。
2. 保持数据库不回滚，除非 migration 明确标注可逆。
3. 执行 `kgctl doctor` 验证 DB / Redis / env。
4. 抽样发 chat 请求，确认 usage / request log 正常。

### 数据库回滚

当前 SQL migration 默认 forward-only。若发布后要回滚：

- 优先应用热修 migration。
- 只有在备份可用且确认无新写入依赖时，才恢复发布前快照。
- 恢复后必须重跑 `kgctl doctor` 与 smoke test。

### 密钥与安全事故

- Master key 丢失：无法解密既有 channel key / OIDC secret；恢复备份或重建密钥并重新录入所有 secret。
- JWT secret 计划轮换：新 key 放 `KOOIX_JWT_SECRET`，旧 key 临时放 `KOOIX_JWT_PREVIOUS_SECRETS`，窗口结束后移除旧 key。
- JWT secret 泄露：立即更换 `KOOIX_JWT_SECRET`，清空 `KOOIX_JWT_PREVIOUS_SECRETS`，重启服务并撤销 session，强制用户重新登录。
- Channel key 泄露：在上游 Provider 轮换 key，Kooix Gate 内撤销旧 `channel_keys` 并录入新 key。
- Redis quota 异常：暂停相关 quota policy，导出 Redis key 与 PG usage 对账，再恢复策略。

## v0.2.0 Smoke Test

```bash
# 1. 基础配置
docker compose config
docker compose -f docker-compose.dev.yml config

# 2. 启动依赖并迁移
docker compose -f docker-compose.dev.yml up -d
export KOOIX_DATABASE_URL=postgres://gate:gate_dev@localhost:5432/gate
export KOOIX_REDIS_URL=redis://localhost:6379/0
export KOOIX_PUBLIC_URL=http://localhost:8000
export KOOIX_MASTER_KEY=$(kgctl key master)
export KOOIX_JWT_SECRET=$(kgctl key jwt)
# export KOOIX_JWT_PREVIOUS_SECRETS=<old kgctl key jwt output>  # planned JWT rotation only
kgctl migrate
kgctl doctor

# 3. 创建管理员并启动服务
kgctl admin create --email root@example.com
cargo run -p gate-server --release
```

发布完成后在 GitHub Actions 确认：

- CI workflow success。
- Docker workflow success。
- GHCR 出现 `v0.2.0` 与 `latest`。
- GitHub Release 页面可见。
