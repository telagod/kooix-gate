// AssemblyScript transform 实战示例：在请求 body 中追加固定 metadata 字段。
//
// Build:
//   npm install
//   npm run asbuild:release
//   sha256sum build/release.wasm

const HEAP_BASE: i32 = 4096;
let bump: i32 = HEAP_BASE;

// @ts-ignore: decorator
@global
export function gate_alloc(size: i32): i32 {
  const p = bump;
  bump += size;
  return p;
}

function encodeReturn(ptr: i32, len: i32): i64 {
  return (i64(ptr) << 32) | i64(len);
}

function returnPayload(payload: Uint8Array): i64 {
  const len = payload.length as i32;
  const ptr = gate_alloc(len);
  for (let i: i32 = 0; i < len; i++) {
    store<u8>(ptr + i, payload[i]);
  }
  return encodeReturn(ptr, len);
}

/// 把 ASCII bytes 转为 String（v0 仅支持 ASCII / UTF-8 BMP）
function bytesToString(buf: Uint8Array): string {
  return String.UTF8.decodeUnsafe(buf.dataStart, buf.length);
}

function stringToBytes(s: string): Uint8Array {
  const utf8 = String.UTF8.encode(s);
  const out = new Uint8Array(utf8.byteLength);
  memory.copy(out.dataStart, changetype<usize>(utf8), utf8.byteLength);
  return out;
}

/// 用户 transform：identity passthrough（同 Rust SDK 示例）
export function chat_request_transform(ptr: i32, len: i32): i64 {
  const buf = new Uint8Array(len);
  for (let i: i32 = 0; i < len; i++) {
    buf[i] = load<u8>(ptr + i);
  }
  // identity — 真实使用可在此 parse JSON 改 messages 等
  return returnPayload(buf);
}

export function chat_response_transform(ptr: i32, len: i32): i64 {
  const buf = new Uint8Array(len);
  for (let i: i32 = 0; i < len; i++) {
    buf[i] = load<u8>(ptr + i);
  }
  return returnPayload(buf);
}
