//! AWS SigV4 + HMAC primitives —— 已经迁移到 `crate::sigv4` 顶层模块。
//!
//! 这个文件保留只是为了兼容 mod tree 结构，所有公共 helper 重新导出。

pub(super) use crate::sigv4::{
    aws_sigv4_signing_key, canonical_query_string, canonical_uri, hmac_sha256_hex,
    infer_aws_region_from_host, sha256_hex,
};
