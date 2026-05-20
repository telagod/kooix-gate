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

# 3. 跑 schema 迁移
export $(grep -v '^#' .env.production | xargs)
kgctl migrate

# 4. 准备好环境后体检（会校验 env / migration / Redis Lua）
kgctl doctor
# CI / deploy pipeline 可用机器可读输出
kgctl doctor --json

# 5. 写入默认模型定价（幂等）
kgctl seed-pricing

# 6. 创建首个 super_admin
kgctl admin create --email root@example.com
# 输出会一次性显示自动生成的 24B 随机初始密码；立即换地方保存

# 7. （之后部署 gate-server 走 K8s / systemd / Docker）

# 8. 服务启动后跑 HTTP 冒烟（登录、建 channel/API key、发 chat、查 usage）
kgctl smoke \
  --base-url https://gate.example.com \
  --email root@example.com \
  --password '<admin-password>' \
  --upstream-base-url https://api.openai.com/v1 \
  --upstream-api-key '<provider-key>'
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
| `doctor` | env 完整性 + DB `SELECT 1` + migration 最新 + Redis `PING` / Lua 一键体检 | `kgctl doctor` |
| `doctor --json` | 同样体检，但 stdout 输出 `{ ok, checks[] }` JSON；失败仍 exit 1 | `kgctl doctor --json` |
| `smoke` | 已运行服务的 HTTP E2E：登录、创建 smoke project/channel/API key、发 chat、查 usage | `kgctl smoke --base-url ... --email ... --password ...` |
| `seed-pricing` | 写入 OpenAI / Anthropic 主流模型默认定价（全局 channel_id NULL） | `kgctl seed-pricing` |
| `pricing list` | 列出 `pricing_rules`，可按 model / channel 过滤 | `kgctl pricing list --model gpt-4o-mini` |
| `pricing set` | 新建一条 global 或 channel-specific 定价规则 | `kgctl pricing set --model gpt-4o-mini --dimension input_tokens --unit per_million --rate 0.15` |
| `pricing delete` | 删除指定定价规则 | `kgctl pricing delete --id <uuid>` |
| `usage-storage plan` | 输出普通 PG 月分区 dry-run SQL | `kgctl usage-storage plan --partition` |
| `usage-storage plan --timescale` | 输出 Timescale hypertable/compression/retention dry-run SQL | `kgctl usage-storage plan --timescale` |
| `plugin schema` | 输出 HTTP Plugin manifest v1 JSON Schema | `kgctl plugin schema` |
| `plugin lint` | 校验 manifest JSON | `kgctl plugin lint manifest.json --base-url https://api.example.com` |
| `plugin test` | 用 manifest 发一次 non-stream chat，验证 request / response mapping | `kgctl plugin test manifest.json --base-url https://api.example.com --model replay-model` |
| `plugin replay` | 回放 raw SSE 并输出归一 chunks | `kgctl plugin replay manifest.json --sse sample.sse --model replay-model` |
| `plugin export` | 导出 manifest golden fixture，可包含 raw SSE expected chunks | `kgctl plugin export manifest.json --sse sample.sse -o fixture.json` |
| `plugin import` | 导入 / 校验 fixture，`--verify` 会重放 raw SSE 比对 expected chunks | `kgctl plugin import fixture.json --verify` |

退出码：成功 0；任何步骤失败 1，标准错误用 ANSI 红色高亮原因。

## HTTP 冒烟测试

`kgctl smoke` 只走公开 HTTP API，不直连数据库。推荐在 `kgctl doctor`、服务启动和上游 mock / 真实 Provider 准备好之后执行：

```bash
export KOOIX_SMOKE_EMAIL=root@example.com
export KOOIX_SMOKE_PASSWORD='<admin-password>'
export KOOIX_SMOKE_UPSTREAM_BASE_URL=https://api.openai.com/v1
export KOOIX_SMOKE_UPSTREAM_API_KEY='<provider-key>'
kgctl smoke --base-url "$KOOIX_PUBLIC_URL"
```

执行链：

1. `POST /v1/auth/login`
2. 若当前用户无 Org 且是 platform admin，则创建 smoke Org；随后创建 smoke Project
3. 若提供 `--upstream-base-url`，创建 OpenAI-compatible channel、channel key、channel group，并设为 Project 默认 group
4. 创建 Project API key
5. 使用新 API key 调 `/v1/chat/completions`
6. 用登录态查 `/v1/usage?range=7d&group_by=day`

不提供 `--upstream-base-url` 时，命令仍会创建 Project/API key，但 chat 会依赖已有默认路由或 `KOOIX_OPENAI_BASE_URL` fallback provider。

## HTTP Plugin 工具

```bash
kgctl plugin schema > plugin-manifest.schema.json
kgctl plugin lint manifest.json --base-url https://api.example.com/v1
KOOIX_PLUGIN_TEST_API_KEY='<provider-key>' \
  kgctl plugin test manifest.json --base-url https://api.example.com/v1 --model replay-model
kgctl plugin replay manifest.json --sse sample.sse --base-url https://api.example.com/v1 --model replay-model
kgctl plugin export manifest.json --sse sample.sse -o fixture.json --base-url https://api.example.com/v1 --model replay-model
kgctl plugin import fixture.json --verify
kgctl plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify --json
kgctl plugin registry list --json
kgctl plugin registry package --id my-private --name "My Private API" --version 1.0.0 --author team \
  --manifest manifest.json -o package.json --tag private --description "private provider"
kgctl plugin registry import package.json --root examples/manifest-registry --allow-unsigned
kgctl plugin registry export --root examples/manifest-registry --include-private
```

`plugin replay` 只做本地归一化：读取 manifest 与 raw SSE fixture，输出 OpenAI-compatible `ChatStreamChunk[]`，用于调试私有 `event:` 分流、`done_path`、tool call delta 与 usage 末帧映射。

`plugin export/import` 是 golden fixture 链路：`export` 会把 manifest、可选 non-stream response sample、raw SSE 与 replay 后的 `expected_chunks` 固化到 JSON；后续升级 schema 或 normalizer 时用 `plugin import --verify` 回放比对，避免私有协议映射漂移。`plugin package lint` 校验目录形态 package，要求 `manifest.json`、`fixtures/*.fixture.json`、`README.md` 与 `security.md` 齐备，`--verify` 会回放 fixture。

`plugin registry` 是 P1.8 manifest registry 链路：`examples/manifest-registry/registry.json` 固化官方 preset、社区 sample 的 id、version、author、sha256、signature kind、兼容范围与 manifest 路径；`package` 把私有 manifest + README/security/fixtures 打包，`import` 写入 private namespace，`export` 默认隐藏 private entries，只有 `--include-private` 才导出。

## 定价规则 CLI

`seed-pricing` 仍是 legacy 默认种子，写入 `model_pricing` 表；运行时多维计费以 `pricing_rules` 为主。日常调价优先使用：

```bash
# 查全局和指定模型规则
kgctl pricing list
kgctl pricing list --model gpt-4o-mini

# 写全局规则
kgctl pricing set \
  --model gpt-4o-mini \
  --dimension input_tokens \
  --unit per_million \
  --rate 0.15 \
  --priority 0 \
  --description "OpenAI global input"

# 写渠道专属规则
kgctl pricing set \
  --model gpt-4o-mini \
  --dimension output_tokens \
  --unit per_million \
  --rate 0.60 \
  --channel-id 019e2c1b-a7d1-7162-8422-07e4b24f5f98

# 删除规则
kgctl pricing delete --id 019e2c1b-a7d1-7162-8422-07e4b24f5f98
```

控制台对应页面为 `/admin/pricing`；REST 对应 `GET/POST /v1/admin/pricing-rules` 与 `DELETE /v1/admin/pricing-rules/:id`。

## 用到的 env

- `KOOIX_DATABASE_URL` — `migrate` / `admin create` / `seed-pricing` / `doctor` 必填
- `KOOIX_REDIS_URL` — `doctor` 必填
- `KOOIX_PUBLIC_URL` — `doctor` 必填，必须是 http/https 根 URL
- `KOOIX_MASTER_KEY` / `KOOIX_JWT_SECRET` — `doctor` 必填
- `KOOIX_JWT_PREVIOUS_SECRETS` — `doctor` 可选；JWT rotation 旧 secret 验签窗口，逗号分隔 base64 ≥32B

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
- doctor 全 env + migration 最新 + Redis Lua / 缺 DB / 缺 public URL / migration pending
- doctor JSON 成功 / 失败机器可读输出
- smoke mock HTTP E2E：login → channel/group/default route → API key → chat → usage
- seed-pricing 首次插入 5 条 + 二次幂等 0 插入 5 跳过
- plugin schema/lint/test/replay/export/import fixture 回归
- `env` 输出包含 `KOOIX_OIDC_DEFAULT_REDIRECT`（SSO 配置回归）
