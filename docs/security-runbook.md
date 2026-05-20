# Security Runbook

## Master key 丢失

`KOOIX_MASTER_KEY` 是 envelope encryption KEK。丢失后无法解密已保存的 channel key、OIDC client secret 等密文。

处置：

1. 先从 KMS / 密码管理器 / 备份恢复原 key。
2. 若无法恢复：冻结新增请求，重新生成 master key，逐个重新录入所有 channel key 与 OIDC secret。
3. 重新运行 `kgctl doctor`，抽样验证 channel 调用。

## Master key 计划轮换

`kgctl key rotate-master` 支持对 `channel_keys.key_enc` 与 `identity_providers.client_secret_enc` 做三段式轮换。

执行链：

```bash
export KOOIX_DATABASE_URL=postgres://...
export KOOIX_MASTER_KEY=<old-base64-32B>
export KOOIX_NEW_MASTER_KEY=<new-base64-32B>

# 1. 只读预检：旧 key 必须能解所有密文
kgctl key rotate-master --dry-run --verify

# 2. 做 PostgreSQL backup/snapshot，确认可恢复

# 3. 写库重加密并用新 key 验证
kgctl key rotate-master --apply --verify

# 4. 切部署环境 KOOIX_MASTER_KEY=<new>，滚动重启全部 gate-server / worker
kgctl doctor
```

回滚：

1. `--apply` 前必须保留 DB backup/snapshot 与旧 master key。
2. 若 verify 失败且服务尚未切新 key，优先恢复 backup；也可在无新写入窗口内用 old/new 对调重新执行一次。
3. 若服务已切新 key 后发现业务异常，先暂停新增 secret 写入，恢复 backup，再恢复旧 `KOOIX_MASTER_KEY` 并重启。

注意：轮换工具不会打印 plaintext secret；输出只包含统计、阶段结果和 rollback plan。

## JWT secret 轮换

`KOOIX_JWT_SECRET` 是 primary signing key；`KOOIX_JWT_PREVIOUS_SECRETS` 是逗号分隔旧 key 验签窗口，只验签、不签发新 token。

正常计划轮换：

1. 使用 `kgctl key jwt` 生成新 secret。
2. 把新 secret 写入 `KOOIX_JWT_SECRET`，把当前旧 secret 追加 / 移入 `KOOIX_JWT_PREVIOUS_SECRETS`。
3. 更新部署环境并重启所有 `gate-server` 实例；用 `kgctl doctor --json` 确认 `KOOIX_JWT_SECRET` 与 `KOOIX_JWT_PREVIOUS_SECRETS` 均通过。
4. 观察最长 refresh TTL 或指定运营窗口。窗口期内旧 token 可继续 refresh，但服务端签发的新 access / refresh token 都使用 primary secret。
5. 窗口结束后从 `KOOIX_JWT_PREVIOUS_SECRETS` 移除旧 secret，再次重启并运行 `kgctl doctor`。

泄露或疑似泄露：

1. 不要把泄露 key 放入 `KOOIX_JWT_PREVIOUS_SECRETS`。
2. 立即替换 `KOOIX_JWT_SECRET`，清空 `KOOIX_JWT_PREVIOUS_SECRETS`，重启所有实例。
3. 撤销受影响用户 session；必要时全局撤销 `user_sessions`，强制重新登录。
4. 复查 access log / audit log，确认泄露窗口内的异常请求。

## Channel key 泄露

1. 先到上游 Provider 控制台撤销泄露 key。
2. 在 Kooix Gate 控制台禁用对应 channel key 或 channel。
3. 录入新 key，执行 health probe / smoke chat。
4. 检查 request logs 中该 channel 的异常调用峰值与费用。

## Admin 高危操作二次确认

以下控制面变更需要 `x-kooix-confirm` header，控制台会显示确认短语：

| 操作 | 确认短语 |
| --- | --- |
| 删除 Channel | `delete:<channel_code>` |
| 轮转 Channel key | `rotate:<channel_code>` |
| 撤销 Channel key | `revoke:<raw_key_uuid>` |
| 停用用户 | `suspend:<email>` |
| 禁用 Channel group | `disable:<group_name>` |
| 新建 / 更新 / 删除 Pricing rule | `pricing:<model>:<dimension>` |

失败会返回 `400 bad_request`，审计不会记录成功变更。成功后 audit log 记录 actor、request_id、IP、User-Agent、before/after diff；diff 会先经过 secret redaction。

## Redis quota 计数异常

1. 暂停受影响 quota policy，避免误伤流量。
2. 导出相关 `quota:*` / `rl:*` key 与 PG `usage_records` 对账。
3. 若 Redis 计数偏大，可按 usage 重新设置；若偏小，优先补充告警而非追扣历史请求。
4. 运行 `kgctl doctor`，确认 Redis PING 与 Lua 脚本可执行。

## HTTP Plugin manifest 风险

Manifest 是不可信输入，尤其是私有 URL、headers、body 模板与 SSE path：

- 禁止在 manifest 中保存明文 secret。
- v0.2.0+ 默认禁止 `request.chat_path` 使用绝对 URL；显式打开必须同时声明 `security.permissions.absolute_urls=true`，并仍拒绝 localhost、link-local、private IP、metadata host 与 DNS rebind。
- 优先配置 `security.outbound_allowlist` 为上游 origin；生产环境继续通过网络策略限制出站目标，阻断代理绕过与内网管理地址 SSRF。
- Header / path / body 模板只允许白名单变量；未知变量应视为错误配置，不得降级放行。
- request body、response body、单个 SSE event 与 `request.timeout_ms` 都有上限；新增私有渠道前按预期返回体调小 limit/timeout。
- 记录错误时必须 redaction：Authorization、api-key、x-api-key、cookie、set-cookie，以及 query 中的 key/token/secret/password。
- 新增私有 manifest 前先保存 request/response/SSE fixture，并在 `security.permissions.secret_slots` 声明实际使用的 secret slot，方便复盘与回放。
