// Kooix Gate WASM Transform SDK — AssemblyScript edition (ADR-0003 v0)
//
// ABI v0:
// - gate_alloc(size: i32) -> i32      bump allocator export
// - chat_request_transform(ptr, len) -> i64
// - chat_response_transform(ptr, len) -> i64
// - stream_chunk_transform(ptr, len) -> i64
//
// 用法：
//   import { exportChatRequest } from "@kooix-gate/wasm-sdk-as";
//   exportChatRequest((input: Uint8Array): Uint8Array => input);

const HEAP_BASE: i32 = 4096;
let bump: i32 = HEAP_BASE;

/// Bump allocator — host 调用 gate_alloc 分配 input buffer。
// @ts-ignore: decorator
@global
export function gate_alloc(size: i32): i32 {
  const p = bump;
  bump += size;
  return p;
}

/// 编码返回值：(ptr << 32 | len)，host 解码。
export function encodeReturn(ptr: i32, len: i32): i64 {
  return (i64(ptr) << 32) | i64(len);
}

/// 把 Uint8Array 写入 wasm 线性内存，返回 i64 编码。
export function returnPayload(payload: Uint8Array): i64 {
  const len = payload.length as i32;
  const ptr = gate_alloc(len);
  for (let i: i32 = 0; i < len; i++) {
    store<u8>(ptr + i, payload[i]);
  }
  return encodeReturn(ptr, len);
}

/// 从 (ptr, len) 读取 input 并传给 callback；输出 Uint8Array 自动写回。
export function withInput(
  ptr: i32,
  len: i32,
  fn: (input: Uint8Array) => Uint8Array,
): i64 {
  const buf = new Uint8Array(len);
  for (let i: i32 = 0; i < len; i++) {
    buf[i] = load<u8>(ptr + i);
  }
  const out = fn(buf);
  return returnPayload(out);
}
