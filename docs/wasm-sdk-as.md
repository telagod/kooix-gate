# AssemblyScript SDK — Kooix Gate WASM Transform

> Status: **v0 package 已落地（0.4.55-0.4.56）**：`sdks/gate-wasm-sdk-as/`（`@kooix-gate/wasm-sdk-as`）+ 示例 `examples/wasm-transform-as/`；npm registry 发布留 v0.5.0+（[G-101](./product-gaps.md#g-101-assemblyscript-sdk-npm-发布)）。

ADR-0003 v0 不限制写 wasm transform 用什么语言；本文档描述用 AssemblyScript（TypeScript-like）写 transform 的最小可用方案。

## 为什么 AssemblyScript

- TypeScript 语法，前端工程师友好
- 直接编译到 wasm32（不需 wasi）
- 与 Kooix Gate ABI v0 兼容（low-level `i32 / i64` ABI）

## 最小示例

### 1. 初始化项目

```bash
mkdir my-transform && cd my-transform
npm init -y
npm install --save-dev assemblyscript
npx asinit .
```

### 2. 实现 `assembly/index.ts`

```typescript
// ABI v0 helpers
const HEAP_BASE: i32 = 4096;
let bump: i32 = HEAP_BASE;

export function gate_alloc(size: i32): i32 {
  const p = bump;
  bump += size;
  return p;
}

function readInput(ptr: i32, len: i32): Uint8Array {
  const buf = new Uint8Array(len);
  for (let i: i32 = 0; i < len; i++) {
    buf[i] = load<u8>(ptr + i);
  }
  return buf;
}

function returnPayload(payload: Uint8Array): i64 {
  const len = payload.length as i32;
  const ptr = gate_alloc(len);
  for (let i: i32 = 0; i < len; i++) {
    store<u8>(ptr + i, payload[i]);
  }
  // (ptr << 32) | len
  return (i64(ptr) << 32) | i64(len);
}

// === 用户 transform：identity passthrough 示例 ===

export function chat_request_transform(ptr: i32, len: i32): i64 {
  const input = readInput(ptr, len);
  // 这里写你的 transform 逻辑
  return returnPayload(input);
}

export function chat_response_transform(ptr: i32, len: i32): i64 {
  const input = readInput(ptr, len);
  return returnPayload(input);
}
```

### 3. 配置 `asconfig.json`

```json
{
  "options": {
    "target": "release",
    "exportRuntime": false,
    "memoryBase": 0,
    "tableBase": 0,
    "use": ["abort=~lib/builtins/abort"]
  },
  "targets": {
    "release": {
      "outFile": "build/transform.wasm",
      "optimizeLevel": 3,
      "shrinkLevel": 2,
      "converge": true,
      "noAssert": true
    }
  }
}
```

### 4. 编译 + 部署

```bash
npx asc assembly/index.ts -o build/transform.wasm --optimize
sha256sum build/transform.wasm
```

然后通过 `kgctl wasm verify build/transform.wasm` 拿 manifest 片段，粘贴到 channel manifest 即可。

## 限制 / Caveat

| 项 | AssemblyScript | Rust SDK |
|----|----------------|----------|
| 模块体积 | 较小（~10-20 KB） | 中（~40-80 KB stripped） |
| 生态 | npm 通用 | crates.io |
| 调试 | console.log via host_log | tracing via host_log |
| JSON parse | 需手写或 `json-as` 库 | serde_json |
| 资源限制 | 同 ADR-0003 v0 hard limits | 同 |

## 进度

- [x] 0.4.49 文档落地
- [x] 0.4.55 `sdks/gate-wasm-sdk-as/` 本地 package（含 `gate_alloc` + `encodeReturn` + `returnPayload` + `withInput` helpers）
- [x] 0.4.56 `examples/wasm-transform-as/` 示例
- [ ] v0.5.0+ `@kooix-gate/wasm-sdk-as` 发到 npm registry（[G-101](./product-gaps.md#g-101-assemblyscript-sdk-npm-发布)）
- [ ] v0.5.0+ asbuild 集成 CI

## 参考

- [ADR-0003 v0](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [Rust SDK (gate-wasm-sdk)](../crates/gate-wasm-sdk/) — 推荐生产用
- [AssemblyScript SDK package](../sdks/gate-wasm-sdk-as/)
- [Example: `examples/wasm-transform-as/`](../examples/wasm-transform-as/)
- [Product gaps § G-101 npm publish](./product-gaps.md#g-101-assemblyscript-sdk-npm-发布)
- [AssemblyScript 官方文档](https://www.assemblyscript.org/)
