//! gate-wasm-sdk: Rust SDK for Kooix Gate WASM transform plugins (ADR-0003 v0)。
//!
//! # 用法
//!
//! ```rust,ignore
//! use gate_wasm_sdk::export_chat_request;
//!
//! export_chat_request!(|body: &[u8]| -> Vec<u8> {
//!     body.to_vec()
//! });
//! ```
//!
//! # ABI v0
//!
//! - `gate_alloc(size: i32) -> i32`：linear memory bump allocator
//! - `chat_request_transform(ptr, len) -> i64`：返回 (ptr_out << 32 | len_out)
//! - `chat_response_transform(ptr, len) -> i64`
//! - `stream_chunk_transform(ptr, len) -> i64`
//!
//! # 编译
//!
//! ```bash
//! cargo build --target wasm32-unknown-unknown --release
//! ```

use std::sync::atomic::{AtomicI32, Ordering};

const HEAP_BASE: i32 = 4096;
static BUMP: AtomicI32 = AtomicI32::new(HEAP_BASE);

#[unsafe(no_mangle)]
pub extern "C" fn gate_alloc(size: i32) -> i32 {
    BUMP.fetch_add(size, Ordering::Relaxed)
}

#[inline]
pub fn encode_return(ptr: i32, len: i32) -> i64 {
    ((ptr as i64 & 0xFFFFFFFF) << 32) | (len as i64 & 0xFFFFFFFF)
}

#[inline]
pub fn return_payload(payload: Vec<u8>) -> i64 {
    let len = payload.len() as i32;
    let ptr = gate_alloc(len);
    unsafe {
        std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr as *mut u8, len as usize);
    }
    encode_return(ptr, len)
}

#[inline]
pub fn with_input<F>(ptr: i32, len: i32, f: F) -> i64
where
    F: FnOnce(&[u8]) -> Vec<u8>,
{
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let output = f(input);
    return_payload(output)
}

#[macro_export]
macro_rules! export_chat_request {
    ($f:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn chat_request_transform(ptr: i32, len: i32) -> i64 {
            $crate::with_input(ptr, len, $f)
        }
    };
}

#[macro_export]
macro_rules! export_chat_response {
    ($f:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn chat_response_transform(ptr: i32, len: i32) -> i64 {
            $crate::with_input(ptr, len, $f)
        }
    };
}

#[macro_export]
macro_rules! export_stream_chunk {
    ($f:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn stream_chunk_transform(ptr: i32, len: i32) -> i64 {
            $crate::with_input(ptr, len, $f)
        }
    };
}

#[cfg(feature = "v1")]
pub mod v1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_return_packs_correctly() {
        let v = encode_return(0x1234, 0x5678);
        assert_eq!(v >> 32, 0x1234);
        assert_eq!(v & 0xFFFFFFFF, 0x5678);
    }
}
