# WASM Plugin ABI vNext 设计稿

Status: **PoC v0 — ADR-0003 收口（2026-05-23）**；wasmtime runtime 留 0.5.0+
Scope: HTTP Plugin manifest v1 稳定后的可执行插件 ABI、sandbox、secret access、determinism、resource limits 与 audit 边界
Last verified: 2026-05-23

> 本设计稿在 0.4.16 升级为 [ADR-0003 v0](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)，
> sample manifest 见 `examples/manifest-registry/community/wasm-transform/0.1.0/`。

## 结论

WASM 插件只作为 **vNext 的受限 transform runtime** 引入，不替代当前 HTTP Plugin manifest v1，也不让插件直接接管路由、计费、鉴权或网络。

推荐边界：

- HTTP Plugin manifest v1 继续是默认扩展面，覆盖绝大多数 Provider / 私有协议接入。
- WASM 插件只处理 manifest 难以表达的 deterministic transform：
  - request transform
  - response transform
  - streaming transform
- Host runtime 继续掌控：
  - network egress
  - secret storage / redaction
  - quota / billing / request log
  - routing / fallback / health
  - audit / metrics / trace
- 插件不得直接访问 filesystem、network、env、clock、random、thread 或进程级全局状态。

## 非目标

- 不在 v0.2.0 实装 WASM sandbox runtime。
- 不开放任意 WASI capabilities。
- 不让 WASM 插件直接发 HTTP 请求、读 DB、写 Redis 或调用 OpenTelemetry exporter。
- 不把 Plugin ABI 设计成通用 FaaS；它只服务 Kooix Gate 的 Provider transform。
- 不在首版支持长期状态、background task、跨请求缓存写入或 streaming side effect。

## 与 HTTP Plugin manifest v1 的关系

WASM 插件挂在 manifest 之后、runtime provider 之前，作为可选 transform layer：

```text
ChatRequest
  -> route / quota / key selection
  -> HTTP Plugin manifest request template
  -> optional WASM request_transform
  -> Host HTTP client + sandbox egress
  -> optional WASM response_transform
  -> ChatResponse / ChatStreamChunk
  -> billing / request log / audit
```

原则：

- manifest 仍声明 base URL、auth strategy、secret slots、outbound allowlist、size limits、probe 与 retry/circuit breaker。
- WASM 插件不能扩大 manifest 权限；只能在 manifest 已声明的 capability / permission 内工作。
- WASM 插件输出仍必须落回现有 `ChatResponse` / `ChatStreamChunk` / `Usage` 规范。
- WASM 插件失败默认 fail-closed；路由层按现有 fallback / cooling_down 策略处理。

## Package 与 registry 形态

首版目录规范建议基于现有 manifest package 扩展：

```text
my-provider-plugin/
  package.json
  manifest.json
  plugin.wasm
  wit/
    kooix-plugin.wit
  fixtures/
    request.json
    non-stream-response.json
    stream.fixture.json
  README.md
  security.md
```

`package.json` 增加：

```json
{
  "kind": "kooix.wasm-plugin",
  "abi_version": "0.1.0",
  "entrypoint": "plugin.wasm",
  "sha256": "sha256:<wasm-digest>",
  "permissions": {
    "request_transform": true,
    "response_transform": true,
    "streaming_transform": true,
    "secret_slots": ["primary"],
    "secret_raw_access": false
  },
  "limits": {
    "memory_pages": 16,
    "fuel": 10000000,
    "wall_timeout_ms": 50,
    "max_input_bytes": 1048576,
    "max_output_bytes": 1048576
  }
}
```

Registry entry 继续记录 id、version、author、signature、compatibility 与 digest；WASM package 必须比纯 manifest package 多校验：

- `plugin.wasm` digest 匹配。
- `abi_version` 在 gateway 支持范围内。
- `security.md` 明确 secret、determinism 与 resource limits。
- fixtures 能回放 request / response / stream transform。
- unsigned package 默认不能导入生产 namespace；测试导入必须显式 `--allow-unsigned`。

## ABI 版本与 WIT 草案

ABI 采用显式 semver，不承诺 0.x 向后兼容：

```text
kooix:plugin/transform@0.1.0
```

WIT 草案只表达稳定方向，字段名以最终 Rust 类型 / JSON Schema 为准：

```wit
package kooix:plugin;

interface host {
  record secret-ref {
    slot: string,
    purpose: string,
  }

  record secret-bytes {
    value: list<u8>,
  }

  variant secret-error {
    denied(string),
    missing(string),
  }

  get-secret: func(ref: secret-ref) -> result<secret-bytes, secret-error>;
  redact: func(value: string) -> string;
  now-ms: func() -> u64;
  nonce: func(bytes: u32) -> list<u8>;
}

interface transform {
  record transform-input {
    request-id: string,
    org-id: string,
    project-id: option<string>,
    channel-id: string,
    model: string,
    json: string,
  }

  record transform-output {
    json: string,
    metadata: string,
  }

  variant transform-error {
    invalid-input(string),
    denied(string),
    upstream-protocol(string),
    internal(string),
  }

  transform-request: func(input: transform-input) -> result<transform-output, transform-error>;
  transform-response: func(input: transform-input) -> result<transform-output, transform-error>;
  transform-stream-event: func(input: transform-input) -> result<transform-output, transform-error>;
  finish-stream: func(input: transform-input) -> result<transform-output, transform-error>;
}
```

宿主传入的是 JSON envelope 字符串而不是 ABI 级复杂结构，避免早期把所有 OpenAI / Provider shape 冻进 WIT。待 ABI 稳定后，再把高频字段提升为 typed records。

## Request transform

输入：

- canonical `ChatRequest` / `EmbeddingRequest` / `ImageGenerationRequest` envelope。
- manifest 展开后的 upstream request draft：
  - method
  - path
  - query
  - headers
  - body
- route context：
  - request_id
  - org_id / project_id
  - channel_id / channel_key label
  - model / mapped_model
  - capability set

输出：

- 修改后的 upstream request draft。
- 可选 opaque metadata；只进入 request log / trace 的 redacted 区域。

约束：

- 不能改写目标 origin；origin 仍由 manifest `base_url` / allowlist 控制。
- 不能新增未声明 secret slot。
- 不能输出超过 `max_output_bytes` 的 body / header。
- header 必须经 host 校验，禁止 CRLF injection。
- query 中疑似 secret 的值必须进入 redaction pipeline。

## Response transform

输入：

- upstream status / headers / body。
- manifest response mapper 之前或之后的 canonical envelope；首版建议 **manifest mapper 之前**，用于处理无法用 path DSL 归一的 vendor shape。

输出：

- OpenAI-compatible `ChatResponse` JSON 或 normalized vendor body。
- usage draft：
  - prompt_tokens
  - completion_tokens
  - cached_tokens
  - reasoning_tokens
  - image_units
  - audio_seconds
  - raw
- error draft：
  - provider_code
  - provider_message
  - normalized_kind
  - retry_after_ms

约束：

- usage 只能减少歧义，不能绕过 billing。缺 usage 时仍走现有 estimated usage fail-closed 逻辑。
- error kind 必须落入 gateway 已知枚举；未知值按 upstream error 处理。
- response metadata 写入前必须经过 allowlist + redaction。

## Streaming transform

Streaming 首版按 event-by-event 处理，不把整条流塞进内存：

```text
raw bytes
  -> Host SSE decoder / chunk boundary
  -> transform-stream-event(event envelope)
  -> ChatStreamChunk | usage-only chunk | done signal
  -> finish-stream()
```

输入 event envelope：

- event name
- data bytes / data JSON
- sequence number
- accumulated byte counters
- model / request context

输出：

- zero or more normalized `ChatStreamChunk`。
- usage-only final chunk。
- done signal。
- optional stream metadata。

约束：

- 单个 event 仍受 `max_sse_event_bytes` 限制。
- 插件不得缓存完整长流；host 只允许 bounded scratch state。
- `finish-stream()` 必须在 timeout / upstream EOF / client cancel 下可安全调用。
- client cancel 不触发插件异步清理；host 只记录 audit 和释放资源。

## Secret access API

默认策略：**尽量不给 raw secret**。

优先使用 host-managed auth：

- bearer / api_key_header / api_key_query
- basic
- hmac
- aws_sigv4
- oauth_client_credentials

若 vNext 确需插件读取 secret：

- package 必须声明 `permissions.secret_slots`。
- manifest channel key 必须存在同名 active slot。
- runtime policy 必须允许 `secret_raw_access=true`，默认 false。
- hostcall 只能在 transform invocation 内使用，返回值不可缓存。
- 每次 secret access 写 audit：
  - request_id
  - plugin id/version/digest
  - channel_id
  - slot
  - purpose
  - allow/deny
- audit 不记录 secret value、derived signature、Authorization header 或 query secret。

推荐额外提供 derived hostcalls，减少 raw secret 暴露：

- `sign-hmac(slot, payload, alg)`
- `aws-sigv4-sign(slot_set, canonical_request)`
- `oauth-token(slot_set, token_url, scope)`
- `redact(value)`

## Deterministic execution constraints

插件执行必须可复现、可审计、可限流：

- 禁止 filesystem、network、env、process、thread、shared memory。
- 禁止直接读取 wall clock；需要时间时只能使用 host 提供的 `now-ms`。
- 禁止直接随机；需要 nonce 时只能使用 host 提供的 `nonce(bytes)`，并进入 trace metadata。
- 禁止不受限浮点依赖；计费用量必须用 integer / decimal 字符串。
- 禁止跨请求持久化写入；实例缓存只允许 immutable compiled module cache。
- 输出必须只由 input envelope + allowed hostcalls 决定。
- 同一 input 在同一 ABI/plugin digest 下应产生同一 transform output，除非显式使用 host nonce/time。

## Resource limits

每次 invocation 的默认 hard limits 建议：

| 资源 | 默认 | 硬上限 | 说明 |
| --- | ---: | ---: | --- |
| wall timeout | 50 ms | 500 ms | request/response transform 不得阻塞主链 |
| stream event timeout | 10 ms | 100 ms | 单 event 小步快跑 |
| memory | 16 pages | 128 pages | 1 page = 64 KiB |
| input bytes | 1 MiB | 16 MiB | 不超过 manifest request/response limit |
| output bytes | 1 MiB | 16 MiB | 超出直接 fail closed |
| fuel / instructions | runtime specific | runtime specific | 防 CPU spin |
| stream scratch state | 256 KiB | 4 MiB | 禁完整流缓存 |

资源超限处理：

- request transform 超限：拒绝本次请求，归一为 plugin runtime error。
- response transform 超限：按 upstream error 处理，不生成伪成功。
- streaming transform 超限：关闭流，写 usage estimate 与 error audit。
- 连续超限进入 channel key failure policy / cooling_down。

## Audit / metrics / trace

WASM runtime 必须输出低基数观测事件。

Audit event：

- `plugin.wasm.load`
- `plugin.wasm.invoke`
- `plugin.wasm.secret_access`
- `plugin.wasm.permission_denied`
- `plugin.wasm.resource_exceeded`
- `plugin.wasm.validation_failed`

Prometheus metrics：

- `plugin_wasm_invocations_total{plugin_id,abi_version,phase,outcome}`
- `plugin_wasm_duration_seconds{plugin_id,abi_version,phase}`
- `plugin_wasm_resource_exceeded_total{plugin_id,resource}`
- `plugin_wasm_secret_access_total{plugin_id,slot,outcome}`

Trace attributes：

- `plugin.id`
- `plugin.version`
- `plugin.digest`
- `plugin.abi_version`
- `plugin.phase`
- `plugin.outcome`

禁止高基数字段进入 metrics label：request_id、org_id、project_id、api_key_id、model 原文、错误原文。

## 验收清单

本设计稿覆盖 `ROADMAP.md` 中 “WASM 插件 ABI 设计稿只做 vNext” 的全部子项：

- [x] request transform：定义输入、输出、权限和 origin 不可扩权规则。
- [x] response transform：定义上游响应、usage、error 归一与 billing fail-closed 边界。
- [x] streaming transform：定义 event-by-event transform、done/usage/finalize 与 cancel 行为。
- [x] secret access API：定义 slot 权限、raw secret 默认禁用、hostcall 与 audit。
- [x] deterministic execution constraints：定义禁止能力、host time/nonce 与输出可复现约束。
- [x] 资源限制与审计：定义 timeout、memory、fuel、bytes、scratch state、audit、metrics、trace。

进入实现阶段前不得只补 runtime crate；必须同时补：

1. `crates/gate-providers` ABI adapter / fixture replay。
2. `crates/kgctl plugin package lint` WASM package 校验。
3. manifest registry 对 WASM package 的 signature / digest 校验。
4. `docs/security-runbook.md` WASM 插件事故处置。
5. gitleaks + malicious fixture regression。
