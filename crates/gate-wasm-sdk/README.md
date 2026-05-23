# gate-wasm-sdk

Rust SDK for writing [Kooix Gate](https://github.com/telagod/kooix-gate) WASM transform plugins (ADR-0003 v0).

> 0.4.21 实装，目标 wasm32-unknown-unknown。

## Quickstart

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
gate-wasm-sdk = { git = "https://github.com/telagod/kooix-gate" }
```

```rust
// src/lib.rs
use gate_wasm_sdk::export_chat_request;

export_chat_request!(|body: &[u8]| -> Vec<u8> {
    // 你的 transform 逻辑
    body.to_vec()
});
```

构建：

```bash
cargo build --target wasm32-unknown-unknown --release
sha256sum target/wasm32-unknown-unknown/release/my_transform.wasm
```

把 wasm 路径 + sha256 粘到 channel `manifest.security.wasm`。

## 导出宏

| 宏 | 对应 hook |
|----|-----------|
| `export_chat_request!` | `chat_request_transform(ptr, len) -> i64` |
| `export_chat_response!` | `chat_response_transform(ptr, len) -> i64` |
| `export_stream_chunk!` | `stream_chunk_transform(ptr, len) -> i64` |

每个宏内部：
1. 自动 `#[unsafe(no_mangle)]` 暴露 hook
2. 从 linear memory 读 input bytes
3. 调用用户闭包
4. 用 `gate_alloc` 分配返回区
5. 编码 i64 (`ptr<<32 | len`)

`gate_alloc(size: i32) -> i32` 由 SDK 提供（bump allocator），host 通过它在 wasm linear memory 内为返回值分配空间。

## 示例

完整 e2e 示例见 [`examples/wasm-transform/`](../../examples/wasm-transform/)。

## 参考

- [ADR-0003 v0](../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [docs/wasm-plugin-abi.md](../../docs/wasm-plugin-abi.md) § ABI v0
- [AssemblyScript SDK](../../sdks/gate-wasm-sdk-as/) — 前端工程师友好替代
- [docs/getting-started.md § WASM Plugin transform](../../docs/getting-started.md#wasm-plugin-transformadr-0003-v0)
