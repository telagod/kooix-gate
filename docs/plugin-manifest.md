# HTTP Plugin Manifest v1

> v0.2.0 先冻结运行期 HTTP Plugin manifest v0；当前主线已进入 v1：固定顶层分区、强类型解析、JSON Schema 与 JSON pointer 错误，为 UI builder / CLI lint / replay harness 共用同一契约。

Kooix Gate 的核心竞争力不是内置多少 Provider，而是让私有协议、认证差异、非标准响应和奇葩 SSE 帧都能通过 manifest 接入，不必重新编译 `gate-providers`。

## 存储位置与启用方式

- Channel 的 `provider_type` 设为 `plugin` / `custom` / `http` / `http_plugin`。
- Manifest 放在 `channels.model_mapping.plugin`。
- 密钥走 `channel_keys` envelope encryption；`channel_keys.label` 就是 manifest 里的 secret slot，空 label / `primary` / `api_key` 都归一为主密钥。本地开发可由 env fallback 注入。
- Manifest 是不可信配置：不得写入明文密钥；已启用 path/query/header/body 模板白名单、绝对 URL 默认禁用、内网/metadata host 拒绝与 body/response/SSE 大小限制。
- v0 旧形态仍自动升级：`{ "plugin": { "preset": { "provider": "openai_compatible" } } }` 会在运行时升级为 `plugin.version = 1` 内部结构。
- JSON Schema 入口：
  - API：`GET /v1/admin/plugin-manifest/schema`
  - CLI：`kgctl plugin schema`
  - lint：`kgctl plugin lint manifest.json --base-url https://api.example.com/v1`

最小 OpenAI-compatible preset：

```json
{
  "plugin": {
    "version": 1,
    "capabilities": { "chat": true, "streaming": true },
    "auth": { "strategy": "bearer", "secret_slot": "primary" },
    "preset": { "provider": "openai_compatible" }
  }
}
```

## 顶层结构 v1

v1 固定以下分区：

```json
{
  "plugin": {
    "version": 1,
    "metadata": {},
    "capabilities": {},
    "auth": {},
    "preset": { "provider": "openai_compatible" },
    "request": {},
    "response": {},
    "stream": {},
    "usage": {},
    "error": {},
    "probe": {},
    "security": {}
  }
}
```

固定分区语义：

- `metadata`：name、vendor、homepage、docs、owner、tags。
- `capabilities`：chat、streaming、tools、embeddings、image、audio、vision、json_mode、batch；与内置 Provider 能力矩阵共用字段，Admin API / 控制台 / 路由都读取同一形状。
- `auth`：认证策略与 secret slot 引用；不允许明文 secret。
- `request`：method、path、query、headers、body、timeout、retry。
- `response`：非流式字段映射。
- `stream`：SSE / chunked streaming 映射。
- `usage`：token / image / audio / cache / batch 归一规则。
- `error`：状态码与错误 body 映射。
- `probe`：健康检查与模型探测。
- `security`：出站 allowlist、大小限制、header redaction。

兼容入口：如果顶层没有 `plugin`，运行时也会接受 `adapter`、`protocol` 或直接把整个对象当作 manifest；未声明 `version` 的旧结构按 v0 自动升级。

## Auth strategy

当前 v1 强类型 schema 已固定策略名：

- `bearer`：默认 `Authorization: Bearer <channel key>`。
- `api_key_header`：必须声明 `header_name`。
- `api_key_query`：必须声明 `query_name`；默认高风险，仅用于必须把 key 放 query 的上游。
- `basic`：必须声明 `username_slot`，password 默认来自 encrypted channel key material。
- `custom_headers`：必须声明 `headers`，只能使用白名单模板变量。
- `hmac`：使用 `secret_slot` 做 HMAC-SHA256 签名，自动注入 timestamp、nonce、signature header。
- `aws_sigv4`：AWS Signature Version 4，请求级签名，自动注入 `Authorization` / `x-amz-date` / `x-amz-content-sha256`。
- `none`：不自动注入认证头。

`secret_slot` / `username_slot` / `password_slot` 是加密材料引用，不是明文 secret。

### Secret slots

运行时按以下顺序取密钥：

1. 若 channel key repo + crypto 已配置，读取该 channel 全部 active `channel_keys`，按 `label` 归一为 slot 后解密注入 plugin runtime。
2. `primary` slot 保持旧行为：优先使用 label 为空、`primary` 或 `api_key` 的 active key；若不存在 primary，则使用当前权重最高的 active key 兼容旧数据。
3. DB 无 active key、repo/crypto 未配置或本地开发时，回退 env：
   - 主密钥：`KOOIX_CH_<CHANNEL_CODE>_KEY`，再退到 `KOOIX_API_KEY`。
   - 非主 slot：`KOOIX_PLUGIN_SECRET_<SLOT>`，slot 会转成大写并把非字母数字替换为 `_`。
   - AWS 兼容 slot：`aws_secret_key` 回退 `AWS_SECRET_ACCESS_KEY`，`aws_session_token` 回退 `AWS_SESSION_TOKEN`。

示例：同一 channel 录入两条 key，`label=primary` 保存默认 Bearer key，`label=basic_user` 保存 Basic username；manifest 可写：

```json
{
  "plugin": {
    "version": 1,
    "auth": {
      "strategy": "basic",
      "username_slot": "basic_user",
      "password_slot": "primary"
    }
  }
}
```

## Provider preset

`preset.provider` 会补齐默认 path、headers、request adapter、response mapper、SSE mapper、capability 默认值与 Base URL 建议。

当前 v0.2.0 支持：

| provider | 说明 |
| --- | --- |
| `openai` / `openai_compatible` | 标准 `/chat/completions`，streaming 自动注入 `stream_options.include_usage=true` |
| `deepseek` / `mistral` / `groq` / `together` / `openrouter` / `moonshot` / `zhipu` / `qwen` / `yi` / `ollama` | OpenAI-compatible 变体 |
| `vllm` / `lm_studio` / `ollama_openai` / `localai` / `xinference` | 本地 / 自托管 OpenAI-compatible endpoint 变体 |
| `azure_openai` | 使用 `/openai/deployments/{{model}}/chat/completions?api-version=...` deployment path，认证走 `api-key` header |
| `gemini` | 使用 Gemini OpenAI-compatible path `/v1beta/openai/chat/completions` |
| `anthropic_messages` | OpenAI messages 转 Anthropic Messages API，含 stream / usage mapper |
| `cohere_chat` | Cohere Chat OpenAI-compatible preset |
| `bedrock_converse` | Bedrock Converse request/response 映射，默认使用 `aws_sigv4` 正式签名 |

Capability 默认值说明：

- OpenAI-compatible preset 默认声明 `chat` / `streaming` / `tools` / `embeddings` / `vision` / `json_mode`；`image` / `audio` / `batch` 需显式确认后再开。
- Anthropic Messages 默认声明 `chat` / `streaming` / `tools` / `vision` / `json_mode`。
- Bedrock Converse 当前声明 `chat` / `streaming`；工具、视觉和结构化输出先按保守能力关闭。
- manifest v1 的 bool 字段无法表达“未声明但显式 false”的三态；preset 只会把 truthy 默认能力并入 manifest，若需要严格禁用能力，应在控制台显示层和路由策略同步检查。

路由行为：chat runtime 会根据已声明能力跳过不满足 stream、tool calling、vision input、JSON mode 的 channel；embedding 路由只选择声明 `embeddings=true` 且有内置 embedding runtime 的 Provider。

示例：Azure OpenAI

```json
{
  "plugin": {
    "version": 1,
    "preset": {
      "provider": "azure_openai",
      "api_version": "2024-02-15-preview"
    }
  }
}
```

## Request mapping

`request.path` 默认只支持相对 `base_url` 的 path，并可使用模板变量；旧字段 `request.chat_path` 仍作为 alias 接受：

```json
{
  "plugin": {
    "version": 1,
    "auth": { "strategy": "custom_headers", "headers": { "X-Api-Key": "{{api_key}}" } },
    "request": {
      "path": "/private/chat/{{metadata.deployment}}",
      "query": { "stream": "{{stream}}", "tenant": "{{metadata.tenant}}" },
      "headers": {
        "X-Model": "{{model}}",
        "X-Tenant": "{{metadata.tenant}}"
      },
      "body": {
        "modelName": "{{model}}",
        "messages": "{{messages}}",
        "prompt": "{{last_user_message}}",
        "tools": "{{tools}}",
        "toolChoice": "{{tool_choice}}",
        "stream": "{{stream}}",
        "limit": "{{max_tokens}}"
      }
    }
  }
}
```

### 模板变量白名单

当前 v1 支持：

- Header 模板：`{{api_key}}`、`{{aws_secret_key}}`、`{{aws_session_token}}`、`{{model}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`、`{{tools}}`、`{{tool_choice}}`、`{{metadata.*}}`、`{{extra.*}}`。
- Path / query 模板：`{{api_key}}`、`{{aws_secret_key}}`、`{{aws_session_token}}`、`{{model}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`、`{{last_user_message}}`、`{{tools}}`、`{{tool_choice}}`、`{{request.*}}`、`{{metadata.*}}`、`{{extra.*}}`。
- Body 模板：`{{api_key}}`、`{{aws_secret_key}}`、`{{aws_session_token}}`、`{{model}}`、`{{messages}}`、`{{tools}}`、`{{tool_choice}}`、`{{metadata}}`、`{{extra}}`、`{{last_user_message}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`、`{{request.*}}`、`{{messages.*}}`、`{{metadata.*}}`、`{{extra.*}}`。

`{{api_key}}` 是运行时解密出的 channel key；`{{aws_secret_key}}` / `{{aws_session_token}}` 只用于显式模板或 AWS 兼容 slot，Bedrock Converse preset 默认走 `aws_sigv4` 签名。

整段占位会保留 JSON 原类型，例如 `"{{stream}}"` 渲染为 boolean；嵌在字符串里则转为字符串。整段占位若解析为 `null`、空字符串、空数组或空对象，会从 query/header/body object 中跳过，用于条件字段。

### Model mapping + deployment path

当 channel 同时需要 plugin manifest 与模型 / deployment 改写时，`model_mapping` 可在 `plugin` 旁声明 `models`（别名：`model_aliases` / `deployments`）：

```json
{
  "plugin": {
    "version": 1,
    "request": {
      "path": "/deployments/{{model}}/chat",
      "body": { "model": "{{model}}", "messages": "{{messages}}" }
    }
  },
  "models": {
    "gpt-4o-mini": "private-mini-deployment"
  }
}
```

请求链路顺序是：project model alias → channel `models` / `model_aliases` / `deployments` → plugin `request.path` / `request.body` 模板。Azure OpenAI 与 Bedrock preset 也走同一条 manifest path。

### Runtime auth 注入

- `auth.strategy = "bearer"`：注入 `Authorization: Bearer <secret_slot>`。
- `auth.strategy = "api_key_header"`：注入 `header_name: <secret_slot>`。
- `auth.strategy = "api_key_query"`：追加 `query_name=<secret_slot>`。
- `auth.strategy = "basic"`：注入 Basic auth；`username_slot` 必填，`password_slot` 未填时使用 `secret_slot`。
- `auth.strategy = "custom_headers"`：按 `auth.headers` 模板注入 header。
- `auth.strategy = "hmac"`：按 `auth.hmac.signed_payload` 渲染签名串，使用 `secret_slot` 计算 HMAC-SHA256，并注入 `timestamp_header`、`nonce_header`、`signature_header`。
- `auth.strategy = "aws_sigv4"`：按 AWS Signature Version 4 生成 canonical request、string-to-sign 与 signing key，并注入 `Authorization`、`x-amz-date`、`x-amz-content-sha256`，若 `session_token_slot` 有值则注入 `x-amz-security-token`。
- `auth.strategy = "oauth_client_credentials"`：用 `auth.oauth` 中的 token endpoint 与 client credential slots 换取 access token，缓存到过期前并注入 `Authorization: Bearer <token>`。
- `auth.strategy = "none"`：不注入认证。

若私有渠道不用 Bearer，推荐走 `auth` 分区，而不是把认证塞进 `request.headers`：

```json
{
  "plugin": {
    "version": 1,
    "auth": { "strategy": "api_key_header", "header_name": "X-Api-Key" }
  }
}
```

### HMAC auth

`hmac` 用于 method/path/body/timestamp/nonce 类私有签名协议。默认签名串：

```text
{{method}}
{{path}}
{{body_sha256}}
{{timestamp}}
{{nonce}}
```

可用模板变量只限：`{{method}}`、`{{path}}`、`{{query}}`、`{{body}}`、`{{body_sha256}}`、`{{timestamp}}`、`{{nonce}}`、`{{request.*}}`。

示例：

```json
{
  "plugin": {
    "version": 1,
    "auth": {
      "strategy": "hmac",
      "secret_slot": "signing",
      "hmac": {
        "signature_header": "X-Signature",
        "timestamp_header": "X-Timestamp",
        "nonce_header": "X-Nonce",
        "signed_payload": "{{method}}\n{{path}}\n{{query}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}",
        "signature_encoding": "hex"
      }
    }
  }
}
```

当前支持：

- `algorithm`: `sha256`。
- `signature_encoding`: `hex` / `base64`。
- `secret_slot` 仍只引用 `channel_keys.label` 或 env fallback，不允许在 manifest 中保存明文 secret。

### AWS SigV4 auth

`aws_sigv4` 用于 Bedrock Runtime 等 AWS API。Bedrock Converse preset 会自动使用：

```json
{
  "plugin": {
    "version": 1,
    "preset": { "provider": "bedrock_converse" },
    "auth": {
      "strategy": "aws_sigv4",
      "aws_sigv4": {
        "service": "bedrock",
        "region": "us-east-1",
        "access_key_slot": "primary",
        "secret_key_slot": "aws_secret_key",
        "session_token_slot": "aws_session_token"
      }
    }
  }
}
```

说明：

- `access_key_slot` 默认 `primary`，通常存 AWS access key id。
- `secret_key_slot` 默认 `aws_secret_key`，回退 env `AWS_SECRET_ACCESS_KEY`。
- `session_token_slot` 默认 `aws_session_token`，回退 env `AWS_SESSION_TOKEN`；为空则不发 `x-amz-security-token`。
- `region` 可显式配置；未配置时会从 `bedrock-runtime.<region>.amazonaws.com` 形式的 host 推断，最后兜底 `us-east-1`。
- 签名头仅包含 `host;x-amz-content-sha256;x-amz-date`，避免把用户可控业务 header 纳入不可预期签名面；出站仍建议用网络 egress 策略限制 AWS 目标。

### OAuth client credentials auth

`oauth_client_credentials` 用于需要先向 IdP 换取 Bearer token 的私有网关。manifest 只声明 slot 名，不保存明文：

```json
{
  "plugin": {
    "version": 1,
    "auth": {
      "strategy": "oauth_client_credentials",
      "oauth": {
        "token_url": "https://idp.example.com/oauth/token",
        "client_id_slot": "client_id",
        "client_secret_slot": "client_secret",
        "scope": "chat:write",
        "audience": "https://api.example.com",
        "expiry_skew_seconds": 60
      }
    }
  }
}
```

说明：

- `token_url` 必须使用 HTTPS；测试环境只允许本地 loopback HTTP。
- `client_id_slot` / `client_secret_slot` 默认分别为 `client_id` / `client_secret`，来源同样是 encrypted `channel_keys.label` 或 env fallback（如 `KOOIX_PLUGIN_SECRET_CLIENT_ID`）。
- token 请求使用 `application/x-www-form-urlencoded`：`grant_type=client_credentials`、`client_id`、`client_secret`，并可选追加 `scope` / `audience`。
- token response 必须含 `access_token`；`token_type` 为空时默认 `Bearer`；`expires_in` 为空时按 3600 秒处理。
- 运行时会缓存 access token，并在 `expires_in - expiry_skew_seconds` 后刷新；`expiry_skew_seconds` 最大 3600。

## Non-stream response mapping

若上游不是 OpenAI-compatible，设置 `response.openai_compatible=false`，再声明字段路径：

```json
{
  "plugin": {
    "response": {
      "openai_compatible": false,
      "id_path": "request.id",
      "model_path": "result.model",
      "content_path": "result.text",
      "reasoning_content_path": "result.reasoning",
      "tool_calls_path": "result.tool_calls",
      "finish_reason_path": "result.finish",
      "request_id_path": "request.id",
      "metadata_path": "vendor",
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output",
        "total_tokens_path": "usage.total",
        "cached_tokens_path": "usage.cache_read",
        "reasoning_tokens_path": "usage.reasoning",
        "image_units_path": "usage.images",
        "audio_seconds_path": "usage.audio_seconds",
        "raw_path": "usage"
      }
    }
  }
}
```

Path 规则：

- `a.b.c` 读取 nested object。
- `choices.0.message.content` 支持 array index。
- `foo.bar|fallback.path|default:""` 表示 first non-null fallback，`default:` / `literal:` 后面必须是合法 JSON literal。
- `.` / `$` 表示整个对象。
- 缺失字段会回退默认值；usage 缺失时按 0 处理，类型不匹配会作为上游 decode error 暴露。

字段语义：

- `reasoning_content_path` 会与 `content_path` 合并到 assistant message 文本里，保持 OpenAI-compatible response shape。
- `tool_calls_path` 必须能反序列化为 OpenAI-compatible `tool_calls` 数组。
- `request_id_path` / `metadata_path` 透传为 `request_id` / `upstream_metadata`，用于日志、replay 与供应商对账。
- `usage.raw_path` 会把 vendor 原始 usage metadata 保留在 response usage 的 `raw` 字段；`image_units_path`、`audio_seconds_path` 暂先进入响应与定价上下文，旧 `usage_records` 投影仍只保存 token / cache / cost。

`finish_reason` 会归一：`stop` / `stopped` / `stop_sequence` / `end_turn` / `done` → `stop`；`max_tokens` → `length`；`tool_use` → `tool_calls`；`safety` → `content_filter`。

## Error / retry / health mapping

非 2xx 上游响应会先经过 manifest error mapper，再进入统一 API error shape 与 channel health 统计：

```json
{
  "plugin": {
    "request": {
      "retry": {
        "max_retries": 1,
        "retryable_status": [429, 500, 502, 503, 504],
        "retryable_codes": ["quota_busy"],
        "cooldown_ms": 30000,
        "circuit_breaker_failures": 3
      }
    },
    "error": {
      "status_path": "vendor_error.status",
      "code_path": "vendor_error.code",
      "message_path": "vendor_error.message",
      "auth_status": [401, 403],
      "rate_limit_status": [429],
      "model_not_found_status": [404],
      "safety_block_codes": ["content_filter", "blocked_by_vendor_policy"],
      "retryable_status": [503],
      "retryable_codes": ["quota_busy"],
      "cooldown_ms": 30000,
      "circuit_breaker_failures": 3
    }
  }
}
```

归一规则：

- auth / invalid key → `authentication_error`。
- rate limit → `rate_limit_error`，保留 `Retry-After` 秒数为毫秒级 retry-after。
- model missing / invalid model → `model_not_found`（`type="invalid_request_error"`）。
- vendor safety / policy block → `policy_error`。
- unknown 5xx / 显式 retryable status/code → retryable upstream error。

运行时行为：

- `request.retry.max_retries` 覆盖 channel `max_retries`；`retryable_status` / `retryable_codes` 会合并到 retry 配置。
- 上游失败会写入 `channel_keys.total_errors` / `consecutive_errors` / `last_error_code`；连续失败达到阈值后 key 进入 `cooling_down` 并设置 `cooldown_until`。
- 路由只选择 healthy 且不在 cooldown 的 key；当前 channel 没有可用 key 时会跳过并尝试同 group 的下一个 channel 或 fallback group。
- gateway 记录 `upstream_errors_total{kind=...}`，便于按错误类型观测和告警。

## Probe / health mapping

Plugin 渠道不再只能走固定 `/models` 探测。`probe` 可声明低成本健康请求，后台 health checker 与 `POST /v1/admin/channels/:id/probe` 共用同一配置：

```json
{
  "plugin": {
    "auth": { "strategy": "bearer" },
    "probe": {
      "model": "tiny-health",
      "path": "/healthz/{{model}}",
      "body": {
        "model": "{{model}}",
        "messages": "{{messages}}",
        "max_tokens": "{{max_tokens}}"
      },
      "success_status": [200, 204],
      "max_cost_micros": 25
    }
  }
}
```

运行时行为：

- 未声明 `probe.path` 时默认 `GET /models`；声明 `probe.body` 后自动切 `POST` 并注入 `Content-Type: application/json`。
- `probe.model` 会进入模板上下文，默认探测消息是单条 `user: Hi`、`max_tokens=1`、`temperature=0`，用于控制成本。
- `success_status` 为空时默认 `[200]`；非成功状态会按现有 health 规则累计失败，`401/403` 直接 auto-disable，`429` 只记录限流。
- `max_cost_micros` 是成本上限声明与 UI/审计提示，不承载密钥或扣费逻辑。
- 成功响应若包含 OpenAI-compatible `data[].id` 或私有 `models[]`，会同步到 channel `supported_models`；恢复探活会把 channel 置回 `active/healthy` 并清理 router metrics。

## SSE stream mapping

共享 SSE decoder 已处理：

- CRLF / LF。
- 注释与 heartbeat。
- 多行 `data:`。
- 分片帧。
- `event:` 分流：`ignore_events` 跳过 heartbeat / ping，`done_events` 把指定事件名视为结束。
- 默认 `[DONE]`，也可用 `done` 自定义结束 token。
- vendor done object：`done_path` + `done_values` 可把 `{"type":"message_stop"}` 一类私有结束对象吞掉。
- 私有 token 帧可映射 `role_path` / `content_path` / `finish_reason_path` / `tool_calls_path` / `usage.*_path`。

私有 SSE 示例：

```json
{
  "plugin": {
    "request": {
      "chat_path": "/private/chat",
      "headers": { "X-Api-Key": "{{api_key}}" },
      "body": {
        "modelName": "{{model}}",
        "prompt": "{{last_user_message}}",
        "stream": "{{stream}}",
        "limit": "{{max_tokens}}"
      }
    },
    "stream": {
      "openai_compatible": false,
      "event_path": "payload",
      "ignore_events": ["ping"],
      "done_events": ["close"],
      "id_path": "rid",
      "model_path": "model_name",
      "role_path": "speaker",
      "content_path": "token",
      "tool_calls_path": "tool_calls",
      "finish_reason_path": "reason",
      "done": ["[DONE]", "EOF"],
      "done_path": "type",
      "done_values": ["message_stop"],
      "usage": {
        "prompt_tokens_path": "usage.in",
        "completion_tokens_path": "usage.out"
      }
    }
  }
}
```

对应原始 SSE：

```text
event: token
data: {"payload":{"rid":"r1","model_name":"m1","speaker":"assistant"}}

data: {"payload":{"token":"he"}}

data: {"payload":{"token":"llo"}}

data: {"payload":{"reason":"done","usage":{"in":3,"out":2}}}

data: {"payload":{"type":"message_stop"}}
```

会归一为 OpenAI-compatible `ChatStreamChunk`，末帧携带 `usage.total_tokens=5`。

Replay harness：

- API：`POST /v1/admin/plugin-manifest/replay`
- CLI：`kgctl plugin replay manifest.json --sse sample.sse --base-url https://api.example.com --model replay-model`
- CLI fixture：`kgctl plugin export manifest.json --sse sample.sse -o fixture.json` 生成 golden；`kgctl plugin import fixture.json --verify` 回放比对 `expected_chunks`。
- CLI package：`kgctl plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify --json` 校验目录规范，要求 `package.json` 指向 `manifest.json`、`fixtures/`、`README.md`、`security.md`，并可回放 `*.fixture.json`。
- CLI registry：`kgctl plugin registry list|package|import|export` 管理 manifest registry。官方/社区入口在 `examples/manifest-registry/registry.json`；每条 entry 固定 `id/version/author/source/manifest_path/sha256/signature/compatibility`。私有包导入写入 `private/<namespace>/<id>/<version>/`，`registry export` 默认不导出 private entries。
- CLI test：`kgctl plugin test manifest.json --base-url https://api.example.com --model replay-model` 发一次 non-stream chat，验证 request / response mapping。
- UI：Channel 创建抽屉提供 7 步 builder：preset/custom → auth → request mapping → response sample 点选字段 → raw SSE replay → probe/test 参数 → 保存并可自动加入 group。编辑抽屉保留 manifest + auth + SSE replay preview。

流式计费门禁：若 upstream 未返回 usage 末帧，gateway 会按请求消息长度与 `max_tokens` 生成 estimated usage，写入 outbox 并用 `raw.estimated=true` 标记，不再静默漏扣。

## Security options

`security` 分区用于 v0.2.0+ 的运行时护栏：

```json
{
  "plugin": {
    "request": {
      "timeout_ms": 30000
    },
    "security": {
      "outbound_allowlist": ["https://api.example.com"],
      "header_redaction": ["authorization", "api-key", "x-api-key"],
      "permissions": {
        "outbound_http": true,
        "absolute_urls": false,
        "oauth_client_credentials": false,
        "secret_slots": ["primary"]
      },
      "max_request_bytes": 1048576,
      "max_response_bytes": 8388608,
      "max_sse_event_bytes": 1048576,
      "allow_absolute_chat_path": false
    }
  }
}
```

- `outbound_allowlist` 是 origin allowlist；为空表示只使用默认 denylist，非空时 base URL、绝对 path 与 OAuth token URL 都必须命中。条目只允许 `http/https` origin（如 `https://api.example.com`），不接受 path/query/fragment。
- 默认 denylist 拒绝 `localhost`、link-local、private IP、unspecified/broadcast IP、`metadata`、`metadata.google.internal` 与 `169.254.169.254`；绝对 URL 与 OAuth token URL 走该 denylist。
- DNS rebind 防护在两层执行：自定义 reqwest DNS resolver 会拒绝解析结果里的内网/metadata IP；响应返回后再检查 `remote_addr`，防止连接目标漂移。
- `header_redaction` 合并默认敏感头（`authorization`、`api-key`、`x-api-key`、cookie、AWS session token 等），用于 probe/debug 输出；query 中包含 key/token/secret/password 的参数在网络错误 URL 中脱敏。
- `permissions.outbound_http` 默认 `true`，关掉会拒绝加载 HTTP plugin；`allow_absolute_chat_path=true` 必须同时声明 `permissions.absolute_urls=true`；`auth.strategy=oauth_client_credentials` 必须声明 `permissions.oauth_client_credentials=true`；`permissions.secret_slots` 非空时会校验所有被 auth 使用的 slot 已声明。
- `request.timeout_ms` 可覆盖 channel timeout，合法范围 1..600000 ms。
- `max_request_bytes` 默认 1 MiB，硬上限 16 MiB。
- `max_response_bytes` 默认 8 MiB，硬上限 64 MiB；非流式响应按实际 body 校验，流式响应先按 `content-length` 提前拒绝并累计 bytes。
- `max_sse_event_bytes` 默认 1 MiB，硬上限 4 MiB。
- `allow_absolute_chat_path` 默认 `false`。即使显式打开，仍会套用 denylist、allowlist 与 DNS rebind 防护。

## 安全边界

v0.2.0 必须遵守：

- Manifest 不保存明文密钥，只能引用 `channel_keys.label` / env fallback 注入的 secret slot。
- Header / path / body 模板只使用白名单变量，未知变量直接拒绝加载。
- `request.chat_path` 默认不接受绝对 URL；显式打开时必须声明 `permissions.absolute_urls=true`，且仍拒绝内网、metadata host 与 DNS rebind。
- request body、response body 与单个 SSE event 都有大小上限。
- 私有 URL / header / body 都视为不可信配置，发布前需要人工 review；生产环境仍建议用网络层 egress firewall 作为 runtime allowlist 外的兜底。
- 不在日志、request log、audit 中写出 `api_key`、secret header、Bearer token 或 query secret；probe/debug 输出必须使用 redacted headers / URL。

后续计划补齐：signed manifest package、跨版本 fixture 批量回放与 WASM runtime PoC。WASM ABI vNext 设计稿见 [wasm-plugin-abi.md](./wasm-plugin-abi.md)。

## 当前测试覆盖

- `cargo test -p gate-providers plugin` / `cargo test -p gate-providers custom_provider`：request template、preset、Azure path、Anthropic adapter、自定义 SSE mapper、绝对 URL/内网 host 拒绝、outbound allowlist、DNS rebind guard、header/query redaction、OAuth permission、request body size limit。
- `cargo test -p gate-providers sse`：共享 SSE decoder 的分片、多行 data、CRLF/LF 行为。
- `cargo test -p kgctl plugin_`：CLI schema/lint/test/replay/export/import golden fixture、directory package lint 与 registry package/import/export。
- `cargo test -p gate-server --test channel_plugin_e2e`：manifest replay → channel create → group binding 控制面闭环。
- `cd web && npm test -- plugin-presets`：前端 preset、auth、builder draft、response path suggestion 与 manifest 生成。
