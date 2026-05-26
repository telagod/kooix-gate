//! WasmBlobStore — fetch wasm module bytes by sha256.
//!
//! 0.4.142（按 product-gaps G-002）：channel manifest 的 `security.wasm.module`
//! 字段是 sha256 内容寻址 URL 或路径。ProviderRouter 启动时迭代 channels，
//! 按 sha256 命中 cache → 未命中走 BlobStore fetch → 写入 cwasm cache（0.4.83）。
//!
//! v0：仅实现 LocalFsBlobStore（从本地 fs 读）。v0.5.x 扩 S3 + OCI artifact。

use async_trait::async_trait;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

/// 抽象 wasm 模块字节流提供者。按 sha256 内容寻址。
#[async_trait]
pub trait WasmBlobStore: Send + Sync {
    /// 取 sha256 对应模块字节。命中返 bytes，未命中返 None，io 错误返 Err。
    async fn fetch(&self, sha256: &str) -> std::io::Result<Option<Vec<u8>>>;

    /// store 的描述名（用于 metric 标签 / log）。
    fn name(&self) -> &'static str;
}

/// LocalFs 实现：从 `{root}/{sha256}.wasm` 读取。
///
/// 适合开发 / 单机部署 / NFS 挂载场景。
pub struct LocalFsBlobStore {
    root: PathBuf,
}

impl LocalFsBlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, sha256: &str) -> PathBuf {
        self.root.join(format!("{sha256}.wasm"))
    }
}

#[async_trait]
impl WasmBlobStore for LocalFsBlobStore {
    async fn fetch(&self, sha256: &str) -> std::io::Result<Option<Vec<u8>>> {
        let p = self.path_for(sha256);
        match tokio::fs::read(&p).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn name(&self) -> &'static str {
        "local_fs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_fs_returns_none_for_missing_sha() {
        let tmp = std::env::temp_dir().join(format!("kooix-blobstore-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let store = LocalFsBlobStore::new(&tmp);
        let result = store.fetch("nonexistent_sha").await.unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn local_fs_returns_bytes_for_existing_sha() {
        let tmp = std::env::temp_dir().join(format!("kooix-blobstore-test-{}-2", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let sha = "abc123";
        let content = b"wasm bytes here";
        std::fs::write(tmp.join(format!("{sha}.wasm")), content).unwrap();

        let store = LocalFsBlobStore::new(&tmp);
        let result = store.fetch(sha).await.unwrap();
        assert_eq!(result.as_deref(), Some(content.as_slice()));

        // 验证 store 标签
        assert_eq!(store.name(), "local_fs");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn local_fs_path_uses_sha_suffix() {
        let store = LocalFsBlobStore::new("/var/cache/wasm");
        let p = store.path_for("deadbeef");
        assert_eq!(p, Path::new("/var/cache/wasm/deadbeef.wasm"));
    }
}
