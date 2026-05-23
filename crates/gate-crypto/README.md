# gate-crypto

Envelope encryption + KMS 抽象，所有上游 channel key / OIDC client_secret 都走这层落库。

## 模块

- `envelope` — AES-256-GCM 信封加密：明文 → DEK 加密 → KEK 包裹 DEK → `(wrapped_dek, ciphertext, nonce, aad)`
- `kms` — KMS trait + `LocalKms`（从 `KOOIX_MASTER_KEY` 派生）+ 留 AWS/GCP/Azure KMS 接入位
- `aad` — Additional Authenticated Data binding helper（绑定 `channel_id` / `identity_provider_id` 防替换攻击）

## 用法

```rust
let kms = LocalKms::from_master_key(master_key)?;
let encrypted = kms.encrypt(plaintext, &aad).await?;
// 落库：(wrapped_dek, ciphertext, nonce, aad)
let plaintext = kms.decrypt(&encrypted, &aad).await?;
```

## 安全约束

- master key 只能从 env `KOOIX_MASTER_KEY`（base64 32B）读
- AAD 必须绑定唯一 row 标识，缺 AAD 解密失败
- `Zeroize` 自动清除栈上明文

故障 / 轮换流程见 [docs/security-runbook.md](../../docs/security-runbook.md)。
