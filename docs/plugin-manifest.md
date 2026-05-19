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
- `capabilities`：chat、streaming、tools、embeddings、image、audio、vision、json_mode、batch。
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
- `none`：不自动注入认证头。

`secret_slot` / `username_slot` / `password_slot` 是加密材料引用，不是明文 secret。

### Secret slots

运行时按以下顺序取密钥：

1. 若 channel key repo + crypto 已配置，读取该 channel 全部 active `channel_keys`，按 `label` 归一为 slot 后解密注入 plugin runtime。
2. `primary` slot 保持旧行为：优先使用 label 为空、`primary` 或 `api_key` 的 active key；若不存在 primary，则使用当前权重最高的 active key 兼容旧数据。
3. DB 无 active key、repo/crypto 未配置或本地开发时，回退 env：
   - 主密钥：`KOOIX_CH_<CHANNEL_CODE>_KEY`，再退到 `KOOIX_API_KEY`。
   - 非主 slot：`KOOIX_PLUGIN_SECRET_<SLOT>`，slot 会转成大写并把非字母数字替换为 `_`。
   - Bedrock 兼容 slot：`aws_secret_key` 回退 `AWS_SECRET_ACCESS_KEY`。

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

`preset.provider` 会补齐默认 path、headers、request adapter、response mapper 与 SSE mapper。

当前 v0.2.0 支持：

| provider | 说明 |
| --- | --- |
| `openai` / `openai_compatible` | 标准 `/chat/completions`，streaming 自动注入 `stream_options.include_usage=true` |
| `deepseek` / `mistral` / `groq` / `together` / `openrouter` / `moonshot` / `zhipu` / `qwen` / `yi` / `ollama` | OpenAI-compatible 变体 |
| `azure_openai` | 使用 `/openai/deployments/{{model}}/chat/completions?api-version=...` deployment path，认证走 `api-key` header |
| `gemini` | 使用 Gemini OpenAI-compatible path `/v1beta/openai/chat/completions` |
| `anthropic_messages` | OpenAI messages 转 Anthropic Messages API，含 stream / usage mapper |
| `cohere_chat` | Cohere Chat OpenAI-compatible preset |
| `bedrock_converse` | Bedrock Converse 基础 request/response 映射；v1 规划正式 `aws_sigv4` auth strategy |

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
      "path": "/private/chat/{{model}}",
      "query": { "stream": "{{stream}}" },
      "headers": {
        "X-Model": "{{model}}"
      },
      "body": {
        "modelName": "{{model}}",
        "prompt": "{{last_user_message}}",
        "stream": "{{stream}}",
        "limit": "{{max_tokens}}"
      }
    }
  }
}
```

### 模板变量白名单

当前 v1 支持：

- Header 模板：`{{api_key}}`、`{{aws_secret_key}}`、`{{model}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`。
- Path / query 模板：`{{api_key}}`、`{{model}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`、`{{last_user_message}}`、`{{request.*}}`。
- Body 模板：`{{api_key}}`、`{{aws_secret_key}}`、`{{model}}`、`{{messages}}`、`{{last_user_message}}`、`{{stream}}`、`{{temperature}}`、`{{top_p}}`、`{{max_tokens}}`、`{{request.*}}`、`{{messages.*}}`。

`{{api_key}}` 是运行时解密出的 channel key；`{{aws_secret_key}}` 仅供现有 Bedrock Converse preset 兼容，正式 SigV4 仍在后续阶段收口。

整段占位会保留 JSON 原类型，例如 `"{{stream}}"` 渲染为 boolean；嵌在字符串里则转为字符串。

### Runtime auth 注入

- `auth.strategy = "bearer"`：注入 `Authorization: Bearer <secret_slot>`。
- `auth.strategy = "api_key_header"`：注入 `header_name: <secret_slot>`。
- `auth.strategy = "api_key_query"`：追加 `query_name=<secret_slot>`。
- `auth.strategy = "basic"`：注入 Basic auth；`username_slot` 必填，`password_slot` 未填时使用 `secret_slot`。
- `auth.strategy = "custom_headers"`：按 `auth.headers` 模板注入 header。
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
      "finish_reason_path": "result.finish",
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output",
        "total_tokens_path": "usage.total",
        "cached_tokens_path": "usage.cache_read"
      }
    }
  }
}
```

Path 规则：

- `a.b.c` 读取 nested object。
- `choices.0.message.content` 支持 array index。
- `.` / `$` 表示整个对象。
- 缺失字段会回退默认值；usage 缺失时按 0 处理。

`finish_reason` 会归一：`stop` / `stopped` / `stop_sequence` / `end_turn` / `done` → `stop`；`max_tokens` → `length`；`tool_use` → `tool_calls`；`safety` → `content_filter`。

## SSE stream mapping

共享 SSE decoder 已处理：

- CRLF / LF。
- 注释与 heartbeat。
- 多行 `data:`。
- 分片帧。
- 默认 `[DONE]`，也可用 `done` 自定义结束 token。

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
      "id_path": "rid",
      "model_path": "model_name",
      "role_path": "speaker",
      "content_path": "token",
      "finish_reason_path": "reason",
      "done": ["[DONE]", "EOF"],
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

data: EOF
```

会归一为 OpenAI-compatible `ChatStreamChunk`，末帧携带 `usage.total_tokens=5`。

## Security options

`security` 分区用于 v0.2.0 的最小运行时护栏：

```json
{
  "plugin": {
    "security": {
      "max_request_bytes": 1048576,
      "max_response_bytes": 8388608,
      "max_sse_event_bytes": 1048576,
      "allow_absolute_chat_path": false
    }
  }
}
```

- `max_request_bytes` 默认 1 MiB，硬上限 16 MiB。
- `max_response_bytes` 默认 8 MiB，硬上限 64 MiB；非流式响应按实际 body 校验，流式响应先按 `content-length` 提前拒绝。
- `max_sse_event_bytes` 默认 1 MiB，硬上限 4 MiB。
- `allow_absolute_chat_path` 默认 `false`。即使显式打开，仍拒绝 `localhost`、link-local、private IP、unspecified/broadcast IP、`metadata`、`metadata.google.internal` 等目标；生产环境仍建议用 egress firewall/allowlist 兜底 DNS rebinding 与运行时解析漂移。

## 安全边界

v0.2.0 必须遵守：

- Manifest 不保存明文密钥，只能引用运行时注入变量。
- Header / path / body 模板只使用白名单变量，未知变量直接拒绝加载。
- `request.chat_path` 默认不接受绝对 URL；显式打开时仍拒绝内网与 metadata host。
- request body、response body 与单个 SSE event 都有大小上限。
- 私有 URL / header / body 都视为不可信配置，发布前需要人工 review；生产环境建议通过网络层限制出站访问，避免 DNS rebinding 或代理绕过。
- 不在日志、request log、audit 中写出 `api_key`、secret header 或 Bearer token。

v1 计划补齐：JSON Schema、auth strategy、出站 allowlist、manifest builder、SSE replay harness 与 signed manifest package。

## 当前测试覆盖

- `cargo test -p gate-providers plugin`：request template、preset、Azure path、Anthropic adapter、自定义 SSE mapper、绝对 URL/内网 host 拒绝、header 模板白名单、request body size limit。
- `cargo test -p gate-providers sse`：共享 SSE decoder 的分片、多行 data、CRLF/LF 行为。
- `cd web && npm test -- plugin-presets`：前端 preset 选择与 manifest 生成。
