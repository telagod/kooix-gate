# WASM Transform sample (ADR-0003 v0 占位)

> Status: **PoC manifest only — runtime 实现未上线**
>
> 此目录是 ADR-0003 v0 的 sample manifest 占位符。`security._wasm_v0_placeholder`
> 字段以下划线前缀确保 0.4.x 的 manifest validator 跳过未知字段。
> 真正的 wasmtime runtime 落地在 0.5.0+，届时此 sample 升级为可执行 PoC。

## 角色

- HTTP plugin manifest 走完 auth / endpoint / response 解析
- 在 request 与 response 路径之间插入 WASM transform hook（v0 仅 chat_request_transform / chat_response_transform / stream_chunk_transform 三个）

## 与 manifest v1 的关系

```text
client request
  → manifest.security.* sandbox 校验
  → manifest.auth.* 注入 header
  → wasm.plugin_chat_request_transform(body)         ← 0.5.0 接通
  → reqwest::post(base_url + chat_path, body)
  → wasm.plugin_chat_response_transform(body)        ← 0.5.0 接通
  → manifest.response.* 抽 content / usage / finish_reason
  → return ChatResponse
```

## 设计参考

- [ADR-0003 WASM Plugin ABI v0](../../../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [WASM 设计稿](../../../../docs/wasm-plugin-abi.md)

## 0.5.0 实现计划

1. `crates/gate-providers/src/wasm_plugin/` 引入 wasmtime
2. `SecurityManifest::wasm_*` 字段从 v0 占位升为 typed field
3. Rust SDK + golden test
4. AssemblyScript / Go SDK 文档（社区跟进）
