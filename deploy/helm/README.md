# Kooix Gate Helm Chart

> `deploy/helm/gate` — production-grade chart for Kubernetes（0.4.31 完善 values + 模板）。

## Install

```bash
# 推荐：先创建 secret
kubectl create secret generic kooix-master-key \
  --from-literal=master_key="$(openssl rand -base64 32)"

kubectl create secret generic kooix-jwt \
  --from-literal=jwt_secret="$(openssl rand -base64 48)"

kubectl create secret generic kooix-postgres \
  --from-literal=dsn="postgres://gate:gate@postgres:5432/gate"

# 安装
helm install gate ./deploy/helm/gate \
  --set master_key.fromSecret=kooix-master-key \
  --set jwt.secret_fromSecret=kooix-jwt \
  --set postgres.dsnFromSecret=kooix-postgres \
  --set redis.url=redis://redis:6379/0 \
  --set public_url=https://gate.example.com
```

## 必填 values

| Key | 说明 |
|-----|------|
| `master_key.fromSecret` 或 `master_key.value` | 32 字节 base64；丢失则 channel keys 全失效 |
| `jwt.secret_fromSecret` 或 `jwt.secret` | HS256 secret（≥ 32 字节 base64） |
| `postgres.dsn` 或 `postgres.dsnFromSecret` | PostgreSQL 连接串 |
| `redis.url` 或 `redis.urlFromSecret` | Redis 连接串 |
| `public_url` | 用户访问入口（OIDC redirect / smoke test） |

## ADR-0003 WASM Plugin runtime

默认 `wasm.enabled=true`。环境变量：

- `KOOIX_WASM_MAX_MEMORY_BYTES`（默认 16 MiB）
- `KOOIX_WASM_MAX_CPU_MS`（默认 50ms）

模块加载由 channel `manifest.security.wasm` 字段触发，无需 chart 配置。

## 升级 / 回滚

```bash
helm upgrade gate ./deploy/helm/gate --set image.tag=v0.4.32
helm rollback gate
```

## 监控

`observability.prometheus.enabled=true` 默认 expose `:9090/metrics`。
Service 自动 expose `metrics` port，可直接 PodMonitor / ServiceMonitor 抓。

## 安全

- `securityContext.readOnlyRootFilesystem=true`（默认）
- `capabilities.drop=[ALL]`（默认）
- 所有 secret 推荐走 `*_fromSecret`，避免 helm release 中明文持久化
