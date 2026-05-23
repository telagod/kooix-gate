# gate-auth

身份认证与会话：Password (Argon2id) / JWT (HS256 + ring) / API Key (SHA-256) / OIDC SSO / `AuthContext`。

## 模块

- `password` — Argon2id hash + verify
- `jwt` — `JwtRing` 双密钥窗口：新 key `KOOIX_JWT_SECRET`，旧 key `KOOIX_JWT_PREVIOUS_SECRETS`（逗号分隔）仅验签
- `api_key` — `sk-kg-...` 前缀 + SHA-256 hash 落库；明文只在创建响应回显一次
- `oidc` — `openidconnect` 4 包装 + `client_secret` envelope 加密 + JIT auto-create
- `session` — refresh session 轮转，每次 refresh 拒绝旧 token 重放
- `context` — `AuthContext` 单一权限门面，外部禁读 raw 角色映射

## 关键边界

- 密码失败计数清零走 `gate-server` admin route；本 crate 只提供 hash/verify
- JWT 轮换：新 key 入 `KOOIX_JWT_SECRET`，旧 key 临时进 `KOOIX_JWT_PREVIOUS_SECRETS`，验签窗口由部署方控制
- API Key：明文不写 audit、不写 log，只回显一次

详见 [DESIGN.md § 2 RBAC 设计](../../DESIGN.md) 与 [docs/security-runbook.md](../../docs/security-runbook.md)。
