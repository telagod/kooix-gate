# Kooix Gate Examples

本目录是 P2.4/P2.5 DX 与发布演示入口，目标是让新用户 10 分钟内完成：配置上游 channel、创建 project API key、按 OpenAI-compatible 协议发起非流式/流式请求，并能补价格、quota、usage / billing 验证。

## 环境变量

```bash
export KOOIX_BASE_URL="http://localhost:8000"
export KOOIX_ADMIN_TOKEN="<login-access-token>"
export KOOIX_ORG_ID="org_..."
export KOOIX_PROJECT_ID="proj_..."
export KOOIX_API_KEY="<project-api-key>"          # Project API key，用于 /v1/chat/completions
export KOOIX_CHANNEL_ID="ch_..."       # 可选：已创建 channel 后填入
export KOOIX_GROUP_ID="grp_..."        # 可选：已创建 group 后填入
export UPSTREAM_BASE_URL="https://api.openai.com/v1"
export UPSTREAM_API_KEY="<provider-key>"
export MODEL="gpt-4o-mini"
```

> `KOOIX_ADMIN_TOKEN` 是 `/v1/auth/login` 返回的 `access_token`。不要把真实 token/key 提交到仓库；示例里的 `<...>` 都是占位。
> pricing 示例会自动发送 `X-Kooix-Confirm: pricing:<model>:<dimension>`，与 Admin 高危操作二次确认一致。

## 一键 demo

发布前或素材录制前可直接跑完整主链路：

```bash
export UPSTREAM_BASE_URL="https://api.openai.com/v1"
export UPSTREAM_API_KEY="<provider-key>"
examples/demo/quickstart.sh
```

脚本会执行 `docker compose up -d --build`、首次 `/v1/setup` 或既有 admin 登录、创建 Provider preset channel、写入 input/output pricing rules、创建 Project API key、发一条 chat，并读取 usage / billing。

## 快速顺序

1. 登录拿 token：见 `collections/bruno/Kooix_Gate/01_Auth/Login.bru` 或 Postman collection。
2. 创建 OpenAI-compatible channel：运行 `admin/create-provider-preset-channel.sh`。
3. 创建 project API key：调用 `/v1/orgs/:org_id/projects/:project_id/api-keys`，或直接跑 `kgctl smoke`。
4. 调用网关：
   - Node OpenAI SDK：`node/openai-sdk-direct.mjs`
   - curl streaming：`curl/streaming-chat.sh`
5. 补运营规则：
   - pricing：`admin/create-pricing-rule.sh`
   - quota：`admin/create-quota.sh`

## 文件索引

| 文件 | 用途 |
| --- | --- |
| `demo/quickstart.sh` | P2.5 一键演示：compose up → setup/login → channel → pricing → API key → chat → usage/billing。 |
| `node/openai-sdk-direct.mjs` | OpenAI SDK 直连 Kooix Gate `/v1/chat/completions`。 |
| `curl/streaming-chat.sh` | `curl -N` 流式 SSE 请求。 |
| `admin/create-provider-preset-channel.sh` | 创建 Provider preset channel、channel key、group、project default group。 |
| `admin/create-pricing-rule.sh` | 创建 `pricing_rules` 规则。 |
| `admin/create-quota.sh` | 创建 Org / Project / ApiKey quota。 |
| `manifests/openai-compatible.json` | 最小 OpenAI-compatible HTTP Plugin manifest。 |
| `manifests/vertex-openai.json` | Google Vertex AI OpenAI-compatible preset manifest，使用 Google Cloud OAuth access token 的 Bearer auth。 |
| `manifests/private-auth-field-map-sse.json` | 私有 auth、字段映射、SSE normalizer 示例。 |
| `manifest-registry/registry.json` | 官方/社区 manifest registry 索引，记录 preset/sample 的版本、作者、sha256、签名与兼容范围。 |
| `manifest-packages/private-auth-field-map-sse/` | P1.8 manifest package 目录规范样本，包含 `manifest.json`、`fixtures/`、`README.md`、`security.md`。 |
| `openapi/kooix-gate.openapi.json` | 轻量 OpenAPI spec，覆盖入门/运营高频接口。 |
| `collections/postman/kooix-gate.postman_collection.json` | Postman collection。 |
| `collections/bruno/Kooix_Gate/` | Bruno collection。 |
| `terraform/` | Docker Compose 风格单机示例部署。 |
| `helm/kooix-gate/` | Helm chart 示例。 |

## smoke 命令

发布后建议直接用 CLI 走完整 HTTP E2E：

```bash
kgctl smoke \
  --base-url "$KOOIX_BASE_URL" \
  --email root@example.com \
  --password '<admin-password>' \
  --upstream-base-url "$UPSTREAM_BASE_URL" \
  --upstream-api-key "$UPSTREAM_API_KEY" \
  --model "$MODEL"
```

该命令会创建 smoke project/channel/group/API key，不做自动清理；生产环境请使用专门的 smoke Org/Project。
