# Quality waiver: legacy large modules

Status: active
Last reviewed: 2026-05-20
Scope: temporary waiver for files that predate the runtime/billing refactor and still exceed the 500 code-line quality threshold.

## Why this waiver exists

The 2026-05-19 refactor first closes runtime separation, outbox concurrency, request-id continuity, usage read models, CI gates, route manifest, and ledger skeleton. Splitting every legacy large module in the same patch would create high import drift and hide the correctness-critical billing changes.

## Current offenders from `checking-code-quality`

- `crates/gate-server/src/routes/admin.rs`
- `crates/gate-server/src/health_check.rs`
- `crates/gate-server/src/middleware/quota.rs`
- `crates/gate-server/src/route_manifest.rs`
- `crates/gate-server/src/routes/chat.rs`
- `crates/gate-server/src/routes/quotas.rs`
- `crates/gate-server/src/routes/sso.rs`
- `crates/gate-storage/src/repo/billing.rs`
- `crates/gate-storage/src/repo/channel.rs`
- `crates/gate-storage/src/repo/channel_key.rs`
- `crates/gate-storage/src/repo/identity.rs`
- `crates/gate-storage/src/repo/memory.rs`
- `crates/gate-storage/src/repo/request_log.rs`
- `crates/gate-storage/src/repo/usage.rs`
- `crates/gate-providers/src/anthropic.rs`
- `crates/gate-providers/src/custom_provider.rs`
- `crates/gate-providers/src/plugin_manifest.rs`
- `crates/gate-providers/src/plugin_preset.rs`
- `crates/gate-providers/src/router.rs`
- `web/src/lib/api.ts`
- `web/src/lib/plugin-presets.ts`

## Exit plan

1. Keep public facades stable.
2. Split by bounded context in small PR-sized slices.
3. Remove each entry from this waiver once its file is below 500 code lines or has a narrower module-level waiver.
4. Continue running `node /home/telagod/.codex/skills/checking-code-quality/scripts/quality_checker.js <path> --json` in review.
