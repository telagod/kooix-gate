# wasm-transform-as

AssemblyScript wasm transform example for Kooix Gate (ADR-0003 v0).

## 编译

```bash
cd examples/wasm-transform-as
npm install
npm run asbuild:release
sha256sum build/release.wasm
```

## 部署

把 `build/release.wasm` 放到 gate 服务可访问路径，更新 channel manifest：

```json
{
  "plugin": {
    "version": 1,
    "preset": { "provider": "openai_compatible" },
    "security": {
      "wasm": {
        "module": "/var/lib/gate/wasm/release.wasm",
        "module_sha256": "<paste-sha256>",
        "max_memory_bytes": 16777216,
        "max_cpu_ms": 50,
        "hooks": ["chat_request_transform", "chat_response_transform"]
      }
    }
  }
}
```

## 与 Rust SDK 示例对比

| | examples/wasm-transform (Rust) | examples/wasm-transform-as (AS) |
|---|---|---|
| 模块体积 | ~40 KB | ~8 KB |
| 依赖 | gate-wasm-sdk + serde_json | 无外部依赖（标准 AS） |
| 实现复杂度 | 高（带 system prompt 注入） | 低（identity passthrough） |
| 推荐用法 | 复杂 transform + JSON parse | 简单 byte-level transform |

详见 [docs/wasm-sdk-as.md](../../docs/wasm-sdk-as.md)。
