# WASM Transform 示例

ADR-0003 v0 用户写 wasm transform 模块的实战示例：在请求 body 中插入 system prompt。

## 编译

```bash
cd examples/wasm-transform
cargo build --target wasm32-unknown-unknown --release

# 模块产物：
# ../../target/wasm32-unknown-unknown/release/wasm_transform_example.wasm

# 计算 sha256（manifest 要写入）
sha256sum ../../target/wasm32-unknown-unknown/release/wasm_transform_example.wasm
```

## 部署

1. 把 `.wasm` 文件放到 gate 服务能访问到的路径（容器内 mount 或 K8s ConfigMap）
2. 在 channel manifest 配置 `security.wasm` 字段：

```json
{
  "plugin": {
    "version": 1,
    "preset": { "provider": "openai_compatible" },
    "security": {
      "wasm": {
        "module": "/var/lib/gate/wasm/wasm_transform_example.wasm",
        "module_sha256": "<paste-sha256-from-step-2>",
        "max_memory_bytes": 16777216,
        "max_cpu_ms": 50,
        "hooks": ["chat_request_transform"]
      }
    }
  }
}
```

3. 通过 control panel / API 创建 channel 时填入此 manifest

## 验证

```bash
# 调用 chat
curl http://gate.example.com/v1/chat/completions \
  -H 'Authorization: Bearer sk-...' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hello"}]}'

# 上游 vendor 收到的实际 body：
# {"model":"gpt-4o-mini","messages":[
#   {"role":"system","content":"You are a careful assistant. Always cite sources."},
#   {"role":"user","content":"hello"}
# ]}
```

## 失败模式

- WASM 加载失败 → 降级到 identity passthrough；`tracing::error!` + Prometheus metric
- 模块 sha256 不匹配 → 拒绝加载，channel 启动失败
- runtime panic / OOM / timeout → 单次 hook 调用 fallback；进程不挂

详见 [ADR-0003](../../docs/architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)。
