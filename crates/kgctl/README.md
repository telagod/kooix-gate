# kgctl

Kooix Gate 部署 / 运维 CLI。

> Sans-server 的纯工具二进制：只读 env、连 DB / Redis、做一次性写入或体检。
> 不暴露任何运行时端口；调一次返回结果即结束。

## 典型部署流程

```bash
# 1. 生成密钥（一次，永久保存到 KMS / 密码管理器）
kgctl init >> .env.production

# 2. 看完整 env 清单，按提示补完 DB / Redis / OIDC / TTL 等
kgctl env

# 3. 准备好环境后体检
export $(grep -v '^#' .env.production | xargs)
kgctl doctor

# 4. 跑 schema 迁移
kgctl migrate

# 5. 写入默认模型定价（幂等）
kgctl seed-pricing

# 6. 创建首个 super_admin
kgctl admin create --email root@example.com
# 输出会一次性显示自动生成的 24B 随机初始密码；立即换地方保存

# 7. （之后部署 gate-server 走 K8s / systemd / Docker）
```

## 子命令一览

| 命令 | 一行说明 | 示例 |
| --- | --- | --- |
| `init` | 一次性生成 master + jwt 两把密钥 | `kgctl init` |
| `key master` | 仅生成 32B AES-256 KEK (base64) | `kgctl key master` |
| `key jwt` | 仅生成 64B JWT HS256 secret (base64) | `kgctl key jwt` |
| `env` | 打印所有部署 env 变量 + 必/可 + 说明 | `kgctl env` |
| `migrate` | 连 `KOOIX_DATABASE_URL` 跑全部 sqlx 迁移 | `kgctl migrate` |
| `migrate --dry-run` | 只列出待执行 migration，不写库 | `kgctl migrate --dry-run` |
| `admin create` | 创建 platform `super_admin` 账号 | `kgctl admin create --email a@b.com` |
| `doctor` | env 完整性 + DB `SELECT 1` + Redis `PING` 一键体检 | `kgctl doctor` |
| `seed-pricing` | 写入 OpenAI / Anthropic 主流模型默认定价（全局 channel_id NULL） | `kgctl seed-pricing` |

退出码：成功 0；任何步骤失败 1，标准错误用 ANSI 红色高亮原因。

## 用到的 env

- `KOOIX_DATABASE_URL` — `migrate` / `admin create` / `seed-pricing` / `doctor` 必填
- `KOOIX_REDIS_URL` — `doctor` 必填
- `KOOIX_MASTER_KEY` / `KOOIX_JWT_SECRET` — `doctor` 必填

完整清单及说明：`kgctl env`。

## 安全注意

- `kgctl init` 生成的密钥**仅显示一次**。`kgctl key master` 与 `kgctl key jwt` 同理。
- `admin create` 在自动生成密码时同样**只打印一次**；丢了得用 `kgctl admin create` 给新邮箱
  再造一个，然后手工撤销旧账号。
- `seed-pricing` 是「插入 + 永久生效」语义；调价不是改这行而是新插一条 + 把旧的
  `effective_until` 闭区间。CLI 不替你做调价，避免误覆盖运行中的计费。

## 集成测试

```bash
# 需要 Docker（用 testcontainers 起 PG 17-alpine + Redis 7-alpine）
cargo test -p kgctl
# CI 上想用别的 tag：
KOOIX_TEST_PG_TAG=17.4-alpine KOOIX_TEST_REDIS_TAG=7.4 cargo test -p kgctl
```

测试覆盖：
- migrate 空库 / dry-run 已迁移库
- admin create 写入 + 同 email 二次报错 + 自动生成密码
- doctor 全 env / 缺 DB
- seed-pricing 首次插入 5 条 + 二次幂等 0 插入 5 跳过
- `env` 输出包含 `KOOIX_OIDC_DEFAULT_REDIRECT`（SSO 配置回归）
