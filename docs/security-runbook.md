# Security Runbook

## Master key 丢失

`KOOIX_MASTER_KEY` 是 envelope encryption KEK。丢失后无法解密已保存的 channel key、OIDC client secret 等密文。

处置：

1. 先从 KMS / 密码管理器 / 备份恢复原 key。
2. 若无法恢复：冻结新增请求，重新生成 master key，逐个重新录入所有 channel key 与 OIDC secret。
3. 重新运行 `kgctl doctor`，抽样验证 channel 调用。

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

## Redis quota 计数异常

1. 暂停受影响 quota policy，避免误伤流量。
2. 导出相关 `quota:*` / `rl:*` key 与 PG `usage_records` 对账。
3. 若 Redis 计数偏大，可按 usage 重新设置；若偏小，优先补充告警而非追扣历史请求。
4. 运行 `kgctl doctor`，确认 Redis PING 与 Lua 脚本可执行。

## HTTP Plugin manifest 风险

Manifest 是不可信输入，尤其是私有 URL、headers、body 模板与 SSE path：

- 禁止在 manifest 中保存明文 secret。
- v0.2.0 默认禁止 `request.chat_path` 使用绝对 URL；显式打开时仍拒绝 localhost、link-local、private IP 与 metadata host。
- 生产环境继续通过网络策略限制出站目标，阻断 DNS rebinding、代理绕过与内网管理地址 SSRF。
- Header / path / body 模板只允许白名单变量；未知变量应视为错误配置，不得降级放行。
- request body、response body、单个 SSE event 都有大小上限；新增私有渠道前按预期返回体调小 limit。
- 记录错误时必须 redaction：Authorization、api-key、x-api-key、cookie、set-cookie。
- 新增私有 manifest 前先保存 request/response/SSE fixture，方便复盘与回放。
