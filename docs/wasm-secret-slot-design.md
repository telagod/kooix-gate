# host_get_secret_slot 设计稿 (ABI v0.x)

> Status: **设计稿（0.4.111）→ 0.5.x 实装**
> 关联：[product-gaps.md G-003](./product-gaps.md#g-003-真实-host-functions-暴露) | [wasm-plugin-abi.md](./wasm-plugin-abi.md) | [ADR-0003 v0](./architecture/decisions/ADR-0003-wasm-plugin-abi-v0.md)

## 为什么要这个

WASM plugin 在 transform hook 里需要拿到 channel 的 secret（API key / OAuth token / SigV4 access_key），用来：

- request transform: 注入 `Authorization: Bearer <key>` 头
- response transform: 解密上游加密 payload
- stream transform: 验签 chunk HMAC

**当前情况**（0.4.65-0.4.110）：插件无法访问任何 secret。manifest `security.permissions.secret_slots` 字段已落 schema（[wasm-plugin-abi.md § 6](./wasm-plugin-abi.md)），但 ABI v0 没暴露读取 fn → 插件唯一选择是放弃 secret 操作 / 重新发明轮子（如把 key 嵌 manifest body template 让宿主拼）。

`host_log` 和 `host_record_metric` 在 0.4.80-0.4.81 实装，三件套就差这个。

## ABI 设计

### 函数签名

```wat
(import "env" "host_get_secret_slot"
  (func $host_get_secret_slot
    (param $name_ptr i32)    ;; UTF-8 slot name 在 wasm linear memory 的指针
    (param $name_len i32)    ;; 长度
    (param $out_ptr i32)     ;; 写回 secret 的 buffer 指针
    (param $out_cap i32)     ;; buffer 容量
    (result i32)))           ;; 写入字节数；负值为错误码
```

### 错误码

| 值 | 含义 |
|----|------|
| `> 0` | 成功，写入字节数 |
| `0` | slot 存在但 secret 为空 |
| `-1` | slot 名 not in manifest `security.permissions.secret_slots` |
| `-2` | slot 已声明但 channel 实际无值（运维忘配） |
| `-3` | out_cap 小于 secret 长度（建议 caller 用 1024 buffer） |
| `-4` | name 越界 / 非 UTF-8 |
| `-5` | host 内部错误 |

### Audit 半生命周期

每次 `host_get_secret_slot` 调用必须 emit audit event：

```rust
audit.emit_change(AuditChange {
    action: "plugin.wasm.secret_access",
    resource_kind: "wasm_secret_slot",
    resource_id: Some(channel_id),
    after: Some(json!({ "slot": slot_name, "size": secret.len() })),
    // 注意：never log secret value, only its length & slot name
});
```

频率削减：同一 (channel, slot) 的连续调用合并到 60s 一条 audit（用 sliding window，避免 hot path log 风暴）。这条 throttling 用 `gate_plugin_wasm_secret_audit_throttled_total` 计数。

### Capability 校验

`WasmHost::load_module` 时把 manifest `security.permissions.secret_slots: ["primary", "secondary"]` 解析成 `HashSet<String>` 存到 `ChannelModule.allowed_slots`。

调用 `host_get_secret_slot("aws_access_key", ...)` 时：
1. 读 name 字符串
2. `if !allowed_slots.contains(&name)` → 返 `-1`
3. 否则查 `channel.secrets[&name]`（从 SharedHttpClient 路径上传过来的）

## Host context 传递路线

当前 `WasmHost::invoke_hook` 签名：

```rust
async fn invoke_hook(
    &self,
    channel_id: &str,
    hook: HookKind,
    payload: Bytes,
    ctx: HookContext,
) -> WasmResult<Option<Bytes>>;
```

`HookContext` 已有 `channel_id`/`model`/`request_id`/`metadata` 字段。需要扩 secrets：

```rust
#[derive(Debug, Clone, Default)]
pub struct HookContext {
    pub channel_id: String,
    pub model: String,
    pub request_id: String,
    pub metadata: HashMap<String, String>,
    /// 0.5.x: 调用方按 channel allowed_slots 过滤后传入。host 不再自己查 manifest。
    pub secrets: HashMap<String, String>,
}
```

调用方（`CustomHttpProvider::with_wasm_host` 路径）在 instantiate hook context 时把 `channel.resolve_secrets()` 结果按 `manifest.security.permissions.secret_slots` 过滤后塞进去——secret 解密发生在调用方，host 拿到的已经是明文 → 减少 host 攻击面。

## Linker 注册

```rust
linker.func_wrap_async(
    "env",
    "host_get_secret_slot",
    |mut caller: Caller<'_, HostState>, args: (i32, i32, i32, i32)| Box::new(async move {
        let (name_ptr, name_len, out_ptr, out_cap) = args;
        let memory = caller.get_export("memory")?.into_memory()?;

        // 1. 读 slot name
        let name = read_utf8(&caller, &memory, name_ptr, name_len)
            .ok_or(/* -4 */)?;

        // 2. 校验 capability
        let secret = caller.data().secrets.get(&name)
            .ok_or(/* -1 if not in allowed_slots, else -2 */)?;

        // 3. 检查 out buffer
        let bytes = secret.as_bytes();
        if (bytes.len() as i32) > out_cap {
            return Ok(-3);
        }

        // 4. 写 wasm linear memory
        memory.write(&mut caller, out_ptr as usize, bytes)?;

        // 5. emit audit (throttled)
        emit_secret_access_audit(&caller.data().channel_id, &name, bytes.len());

        Ok(bytes.len() as i32)
    }),
)?;
```

`HostState` 替换 `()`：每次 invoke_hook 创建 Store 时把 HookContext 的 secrets map 塞到 store data 里。

## SDK side（gate-wasm-sdk-rs）

```rust
// crates/gate-wasm-sdk/src/lib.rs
pub fn get_secret_slot(name: &str) -> Result<String, SecretError> {
    const BUF_SIZE: usize = 1024;
    let mut buf = vec![0u8; BUF_SIZE];
    let n = unsafe {
        host_get_secret_slot(
            name.as_ptr() as i32,
            name.len() as i32,
            buf.as_mut_ptr() as i32,
            BUF_SIZE as i32,
        )
    };
    match n {
        n if n > 0 => {
            buf.truncate(n as usize);
            String::from_utf8(buf).map_err(|_| SecretError::InvalidUtf8)
        }
        0 => Ok(String::new()),
        -1 => Err(SecretError::SlotNotAllowed(name.to_string())),
        -2 => Err(SecretError::SlotEmpty(name.to_string())),
        -3 => Err(SecretError::BufferTooSmall),
        _ => Err(SecretError::HostError(n)),
    }
}
```

## 验收门禁（v0.5.x 实装时）

- [ ] examples/wasm-transform-secret-access/ — 完整 demo plugin（接 secret 拼 Authorization 头）
- [ ] e2e test: manifest 声明 `secret_slots: ["primary"]`，plugin 读 `primary` 成功 + 读 `secondary` 拒绝（-1）
- [ ] audit log: 命中条目 `action="plugin.wasm.secret_access"` + `after.slot="primary"` + 不含 secret 值
- [ ] metric: `gate_plugin_wasm_secret_access_total{slot}` + `gate_plugin_wasm_secret_audit_throttled_total`
- [ ] doc: docs/wasm-plugin-abi.md § host_get_secret_slot 段（错误码表 + audit 节奏 + capability 模型）

## 不做什么

- **不做 secret 缓存**：每次 invoke_hook 都重新从 channel.secrets map 拿。host 不持有 secret 跨 hook 调用。
- **不做 OAuth refresh**：plugin 拿到的 secret 是已 resolve 的明文。OAuth flow 在调用方（CustomHttpProvider）完成。
- **不做 secret 写入**：plugin 只读，不能 set_secret。轮换/旋转走 admin endpoint。
- **不做 cross-channel secret 共享**：每个 channel 实例的 secrets 独立，plugin 看不到其他 channel 的 secret。

## 决策原因

product-gaps G-003 标 "v0 仅 ADR-0003 § v0 host functions 列出，未实装"。本设计把：

1. 错误码语义钉死（防 SDK 各自实现）
2. Audit / metric 节奏钉死（防 hot path log 风暴）
3. Host context 传递机制钉死（secret 解密发生在调用方，host 拿明文不再自行查 manifest）
4. SDK 接口钉死（让 ABI v0.x → v1 wit-bindgen 迁移时有形状参照）

实装在 v0.5.x（涉及 HookContext schema 改动 + WasmtimeHost data type 替换 + audit 链路 + 4 个新 metric，超出 patch 范围）。

---

*Designer: 邪修红尘仙 / Date: 2026-05-26 / 关联 commit: 0.4.111*
