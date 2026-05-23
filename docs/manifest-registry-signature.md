# Manifest Registry Signature Schema

> 0.4.53 起 typed signature schema 正式落地。SigstoreBundle / Cosign / Minisign 三种支持。

## 字段

`registry.json` entries[].signature:

```json
{
  "kind": "cosign" | "minisign" | "sigstore_bundle" | "unsigned",
  "value": "<base64-encoded signature bytes>",
  "key_id": "<optional: public key fingerprint / certificate identity>",
  "alg": "<optional: ed25519 | rsa-pss-sha256 | ecdsa-p256-sha256>"
}
```

## kind 详解

### cosign

工具：[cosign](https://github.com/sigstore/cosign)（keyless or with key）

```bash
# Keyless（推荐，OIDC keyless signing）
cosign sign --yes path/to/manifest.json

# 把 sig + cert 一起放到 registry
sig=$(cosign sign-blob --yes path/to/manifest.json 2>/dev/null | base64 -w0)
cert=$(cosign sign-blob --output-certificate /tmp/cert path/to/manifest.json && base64 -w0 /tmp/cert)
```

registry.json 段：

```json
"signature": {
  "kind": "cosign",
  "value": "<base64-sig>",
  "key_id": "<base64-cert>",
  "alg": "ecdsa-p256-sha256"
}
```

### minisign

工具：[minisign](https://jedisct1.github.io/minisign/)

```bash
minisign -Sm path/to/manifest.json -s ~/.minisign/seckey
# 产物：manifest.json.minisig
```

registry.json 段：

```json
"signature": {
  "kind": "minisign",
  "value": "<base64 of manifest.json.minisig>",
  "key_id": "<base64 of pubkey>"
}
```

### sigstore_bundle

工具：cosign 输出的 bundle 模式（含 cert + sig + rekor 入链证明）

```bash
cosign sign-blob --bundle bundle.sig path/to/manifest.json
```

registry.json 段：

```json
"signature": {
  "kind": "sigstore_bundle",
  "value": "<base64 of bundle.sig>"
}
```

### unsigned

仅 dev / private 场景。`kgctl plugin registry verify` 默认拒绝；`--allow-unsigned` 才放行。

## 验证

```bash
# 严格验签
kgctl plugin registry verify <entry-id>

# Allow unsigned（PoC / dev）
kgctl plugin registry verify <entry-id> --allow-unsigned
```

## 当前实现进度

| 字段 | 0.4.53 | 0.5.0+ |
|------|--------|--------|
| schema typed (`kind` enum) | ✅ | — |
| `value` / `key_id` / `alg` 字段 | ✅ | — |
| `validate_signature` schema 校验 | ✅ | — |
| cosign 真实验签（调用 cosign CLI 或 sigstore-rs） | placeholder | 真实链接 |
| minisign 验签（调用 minisign CLI / 内置） | placeholder | 内置 ed25519 |
| sigstore_bundle 验签（rekor inclusion proof） | placeholder | sigstore-rs |

## Trust chain

```
仓库 maintainer keypair
  ↓ sign
manifest.json + sha256 → signature
  ↓ ingest
registry.json entry
  ↓ kgctl plugin registry import <pkg>
  → validate_signature(strict)
  → check sha256
  → save to channel.security.wasm
```

## 参考

- [ADR-0003 v0 § Trust chain](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)
- [Threat model § 0.4.x WASM 新增表面](./threat-model.md)
