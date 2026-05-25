# WASM Plugin Runbook

> ADR-0003 v0 / 0.4.x 起 WASM transform 上线后的故障处理手册。

## 1. WASM 模块加载失败

**症状**：channel 启动报错 / `kgctl doctor` WASM_RUNTIME 通过但 channel 健康检查失败。

**诊断**：

```bash
# 看日志
kubectl logs deploy/gate | grep 'wasm module'

# 检查 sha256
sha256sum /var/lib/gate/wasm/<module>.wasm
# 对比 channel.security.wasm.module_sha256
```

**常见原因**：

- `DigestMismatch`：文件被替换/损坏。重新上传正确版本。
- `Load: compile`：wasm 二进制不合法。重新编译 `cargo build --target wasm32-unknown-unknown --release`。
- 路径错误：容器内 mount 缺失。检查 ConfigMap / PVC 挂载。

**处置**：

```bash
# 临时禁用 wasm，channel 走 manifest 老路径
kgctl plugin disable-wasm --channel <id>

# 或全局关 wasm runtime（不推荐生产）
KOOIX_WASM_DISABLED=1
```

## 2. WASM hook 频繁超时 / OOM

**症状**：`gate_plugin_wasm_calls_total{status="timeout"}` 或 `{status="oom"}` 速率 > 5%。

**诊断**：

```bash
# Prometheus
rate(gate_plugin_wasm_calls_total{status=~"timeout|oom"}[5m])
  / rate(gate_plugin_wasm_calls_total[5m])
```

**处置**：

1. 提升模块限制（修改 channel manifest）：
   ```json
   "wasm": {
     "max_memory_bytes": 33554432,   // 32 MiB
     "max_cpu_ms": 100               // 100 ms
   }
   ```
2. 长期看，模块写错优化（避免大循环 / 大内存复制）
3. 实在不行：disable hook，转回 manifest dot-path 实现

## 3. WASM panic 暴风雨

**症状**：`gate_plugin_wasm_calls_total{status="panic"}` 突然飙升。

**fallback 行为**：所有 panic 都降级为 identity passthrough，**用户请求不会失败**。

**处置**：

1. 不必紧急止血：业务不受影响
2. 拉日志看 panic 内容
   ```bash
   kubectl logs deploy/gate | grep 'wasm hook panicked' | head -20
   ```
3. 临时关 wasm 走 manifest 老路径
4. 修模块 → 重 build → 重新上传 → 更新 sha256

## 4. 上游全挂

参考 [observability-runbook.md § 上游](./observability-runbook.md)。

WASM 模块对上游错误不感知，纯 transform 层。上游 5xx 走 provider error mapper。

## 5. Redis 不可用

quota 计数走 Redis。Redis 宕：

1. Redis Sentinel 自动 failover（如已部署）
2. quota check fail-closed（拒绝新请求）或 fail-open（视配置）
3. WASM 不受影响（无 Redis 依赖）

## 6. 版本回滚（含 wasm 模块）

```bash
# 回滚 gate 服务
helm rollback gate

# 回滚 wasm 模块：恢复旧 sha256 版本，更新 channel manifest
kgctl plugin update-channel-wasm --channel <id> \
  --module /var/lib/gate/wasm/<module>.wasm \
  --sha256 <old-hex>
```

## 7. cwasm 持久化缓存（0.4.83 起）

启用 `KOOIX_WASM_CACHE_DIR` 后，gate-server 把 wasmtime 编译结果序列化到 disk，第二次冷启动直接 `Module::deserialize_file` 不再 compile。

```bash
# 启用
export KOOIX_WASM_CACHE_DIR=/var/cache/kooix-gate/wasm

# 路径约定
ls $KOOIX_WASM_CACHE_DIR
# {sha256-hex}-wt26-0.cwasm
```

### 运维要点

- **wasmtime 升级**：cwasm 文件名带 `wt26-0` 是 wasmtime major 标记。升级 wasmtime（例：26 → 27）时**所有旧 cwasm 自动失效**——会 deserialize 失败 → 自动 fallback compile + 重写。无需手工清理。
- **wasm 模块更新**：新 sha256 产生新 cwasm 文件，旧文件不会自动清理。可周期性清理：
  ```bash
  # 删 30 天前的 cwasm（按 atime）
  find $KOOIX_WASM_CACHE_DIR -name '*.cwasm' -atime +30 -delete
  ```
- **cache miss 抖动**：观察指标 `gate_wasm_cache_miss_total` / `gate_wasm_cache_corrupt_total`。
  正常运营时 miss 应只在首次冷启或新模块上线时出现，corrupt > 0 触发告警（cwasm 文件损坏 = 磁盘问题或 wasmtime 版本错位）。
- **多 replica 共享**：cwasm 文件 byte-identical（sha256 内容寻址），可挂 ReadWriteMany PVC 让所有 replica 共享同一 cache（避免每个 replica 各自 compile）。

## 联系人

- ON-CALL: oncall@example.com
- WASM module 提交人：见 channel.audit_log 创建者字段
