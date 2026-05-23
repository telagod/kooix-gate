# @kooix-gate/wasm-sdk-as

AssemblyScript SDK for Kooix Gate WASM transform plugins (ADR-0003 v0).

## Quickstart

```bash
npm install --save-dev assemblyscript
# 项目结构：
# my-transform/
#   assembly/
#     index.ts
#   asconfig.json
#   package.json
```

`assembly/index.ts`:

```typescript
import { withInput } from "@kooix-gate/wasm-sdk-as";

export function chat_request_transform(ptr: i32, len: i32): i64 {
  return withInput(ptr, len, (input: Uint8Array): Uint8Array => {
    // Your transform here
    return input;
  });
}
```

Build:

```bash
npm run asbuild:release
sha256sum build/release.wasm
```

Then paste `release.wasm` path + sha256 into channel `manifest.security.wasm`.

## API

| Export | 说明 |
|--------|------|
| `gate_alloc(size)` | bump allocator，host 用 |
| `chat_request_transform(ptr, len) -> i64` | 用户实现 |
| `chat_response_transform(ptr, len) -> i64` | 用户实现 |
| `stream_chunk_transform(ptr, len) -> i64` | 用户实现 |

| Helper | 说明 |
|--------|------|
| `withInput(ptr, len, fn)` | 自动读 input + 调用 fn + 写出 |
| `encodeReturn(ptr, len)` | i64 编码 |
| `returnPayload(buf)` | 写 Uint8Array 到 linear memory |

## 参考

- [ADR-0003 v0](../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [Rust SDK](../../crates/gate-wasm-sdk/) — 推荐生产用
- [docs/wasm-sdk-as.md](../../docs/wasm-sdk-as.md) — 详细教程
