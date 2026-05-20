# Kooix Gate Threat Model

Status: active  
Last verified: 2026-05-20  
Scope: control plane, data plane, provider plugin runtime, billing and admin operations.

## Assets

- Tenant data: org / project / user membership, quota policy, pricing rule, billing ledger, request logs.
- Secrets: project API keys, channel upstream keys, OIDC `client_secret`, JWT signing keys, `KOOIX_MASTER_KEY`.
- Runtime integrity: provider routing, HTTP Plugin manifest execution, quota / billing settlement, audit logs.
- Admin authority: platform admin sessions, high-risk mutations, release and migration credentials.

## Trust boundaries

| Boundary | Untrusted side | Trusted side | Main controls |
| --- | --- | --- | --- |
| Public API | Client HTTP request, headers, body | `gate-server` handlers and middleware | auth loader, RBAC, rate/quota middleware, typed IDs, request-id trace |
| Data plane -> Provider | Model input, plugin manifest mapping, upstream errors | provider router, billing / request log emit | capability routing, key cache invalidation, redaction, size/timeout limits |
| Control plane -> DB | Admin mutation body | repository layer and migrations | permission checks, high-risk confirmation, audit before/after, SQL parameters |
| Secret storage | Stored ciphertext | envelope KMS and AAD-bound decrypt | `KOOIX_MASTER_KEY`, `aad::channel_key(channel_id)`, `aad::idp_secret(id)` |
| Plugin manifest | User-supplied JSON | normalized runtime provider | schema lint, SSRF guard, header/query redaction, fixture replay |

## Threats and controls

### Tenant isolation

**Threats**

- Cross-org access by replaying typed IDs from another tenant.
- Data-plane request using a project key to read or mutate control-plane resources.
- Audit / billing / usage views leaking rows across org boundaries.

**Controls**

- Authorization stays behind `AuthContext` + `require!` checks; platform, org and project permissions are not read from raw membership maps.
- URL path IDs accept typed and raw UUID through `FlexUuid`, but authorization is enforced by subject scope rather than prefix text.
- Audit, usage, billing and request-log query paths carry org/project context and use repo-level filters / RLS context where PostgreSQL is involved.
- Data-plane routers are separated in runtime modes; control-plane admin routes require platform permissions.

**Verification**

- `cargo test -p gate-server --test auth_flow admin_audit_logs_support_pagination_and_sort_query`
- `cargo test -p gate-server --test x_kooix_project`

### API key leakage

**Threats**

- Channel key, OIDC secret or bearer token appears in audit logs, API error responses, debug probes, frontend state or release artifacts.
- Query-string secrets leak through upstream network errors.
- A copied ciphertext is replayed under another channel / IdP context.

**Controls**

- Channel keys and OIDC secrets use envelope encryption; AAD binds channel keys to `channel_id` and IdP secrets to `identity_provider.id`.
- `audit_redaction` redacts sensitive JSON keys and common token prefixes before audit diff persistence.
- Provider error messages are sanitized before HTTP response and failure policy recording.
- High-risk key rotation / revoke operations audit fingerprints and key metadata only, never plaintext or ciphertext.
- `kgctl key rotate-master` supports dry-run, re-encrypt, verify and rollback plan for KEK rotation.

**Verification**

- `cargo test -p gate-server audit_redaction`
- `cargo test -p gate-server provider_error_message_redacts_api_keys_and_bearer_tokens`
- `gitleaks detect --source . --redact --verbose`

### Malicious plugin manifest

**Threats**

- Manifest attempts SSRF, DNS rebinding, private metadata access or absolute URL escape.
- Manifest injects forbidden headers, oversized bodies, unbounded SSE stream or unexpected auth material.
- Private protocol mapping causes silent billing omission or replay drift.

**Controls**

- Manifest schema / CLI lint normalize preset and auth strategy before runtime use.
- Absolute URL use is denied unless explicitly permitted, and still rejects localhost, private, link-local and metadata hosts with DNS rebind checks.
- Header template variables, body size, response size, SSE event size and timeout are bounded.
- Secret slots are resolved from encrypted channel keys / env fallback; manifest never stores plaintext secrets.
- Golden fixtures and `kgctl plugin replay|import --verify` lock request/response/SSE behavior across upgrades.

**Verification**

- `kgctl plugin lint examples/manifests/openai-compatible.json --base-url https://api.example.com/v1`
- `kgctl plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify --json`

### SSRF

**Threats**

- Admin creates a channel or plugin manifest pointing at internal metadata services.
- OAuth token URL / request path bypasses base URL allowlist.
- DNS response changes after validation.

**Controls**

- Plugin manifest validation treats outbound URL fields as untrusted and enforces base URL / allowlist / private-network rejection.
- Runtime HTTP client validates resolved peer address and blocks DNS rebinding to private ranges.
- Production deployment should add egress network policy as defense-in-depth.

**Verification**

- `cargo test -p gate-providers custom_provider`
- `kgctl plugin test <manifest> --base-url https://api.example.com/v1`

### Billing fraud

**Threats**

- Missing usage in streaming / multimodal routes causes free requests.
- Duplicate outbox events double-charge users.
- Admin changes pricing without traceability.
- A bad pricing sync pollutes global rules.

**Controls**

- Data-plane emit path writes usage to billing outbox; stream fallback records estimated usage when final usage is absent.
- Outbox settlement is idempotent by request id / idempotency key, and duplicates are marked done without duplicate ledger writes.
- Pricing rules are high-risk operations: require `x-kooix-confirm: pricing:<model>:<dimension>` and emit before/after audit.
- Billing ledger is the audit source of truth; usage records are read-model projections.

**Verification**

- `cargo test -p gate-server --test billing_e2e`
- `cargo test -p gate-billing --test outbox_consumer`

### Admin account takeover

**Threats**

- Compromised admin session deletes channels, rotates/revokes keys, suspends users, disables groups or changes pricing silently.
- Stolen refresh token persists after incident.
- Audit lacks request trace metadata.

**Controls**

- Admin routes require platform scope and operation-specific permission checks.
- High-risk operations require an explicit confirmation header tied to the resource (`delete:<channel_code>`, `rotate:<channel_code>`, `revoke:<key_uuid>`, `suspend:<email>`, `disable:<group_name>`, `pricing:<model>:<dimension>`).
- Audit records include actor subject, request_id, actor IP, user-agent, before and after snapshots.
- Session management supports per-session and all-session revoke; JWT rotation separates primary and previous secrets.

**Verification**

- `cargo test -p gate-server --test admin_users_e2e platform_admin_can_create_list_suspend_and_reset_password`
- Review `/admin/audit` expanded rows for request_id / IP / user-agent / before / after.
