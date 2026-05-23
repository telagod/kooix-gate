# gate-cache

Redis 抽象层：rate limit + quota 计数 + key decrypt 短缓存。基于 `fred` 9（rustls + i-scripts）。

## 关键能力

- **Lua 原子脚本**：rate limit (`rpm` / `tpm`) + quota check + concurrent slot acquire/release，所有在同一 EVAL 内完成，避免 race
- **crash-safe pre-debit**：budget quota 先 Redis 预扣 → 写 `inflight_requests` → 正常 settle 多退少补 / 异常 drop 全退 / 进程崩溃由 sweeper 兜底
- **channel key 短缓存**：`KOOIX_CHANNEL_KEY_CACHE_TTL_SECS`（默认 30s，0 禁用）

## 模块

- `client` — `fred::Pool` 包装
- `rate_limit` — Lua 实现的 sliding window
- `quota` — Lua quota check + pre-debit + settle
- `concurrent` — concurrent slot 计数

## 故障模式

- Redis 不可用：quota check fail-closed（拒绝新请求）；具体策略见 [docs/observability-runbook.md](../../docs/observability-runbook.md)
- Lua 脚本 SHA 失效：自动 EVAL fallback

详见 [DESIGN.md § 3 配额](../../DESIGN.md) 与 [docs/architecture/data-plane.md](../../docs/architecture/data-plane.md)。
