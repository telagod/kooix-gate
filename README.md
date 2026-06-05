<p align="center">
  <img src="./web/src/lib/assets/kooix-logo.svg" alt="Kooix Gate logo" width="128" height="128" />
</p>

# Kooix Gate

**A multi-tenant Rust LLM gateway that gets billing, quotas, and access control right — before worrying about provider count.**

Add a new provider with a JSON manifest in 5 minutes, no rebuild. Fail-closed streaming billing. Multi-org row-level security. Compile-time SQL. Strongly typed IDs.

[中文版 README](./README.zh.md)

[![Tests](https://img.shields.io/badge/tests-556%2B%20Rust%20%2B%20127%20web-brightgreen)](#testing)
[![Rust](https://img.shields.io/badge/rust-2024-orange)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.5.0--rc2-blue)](./CHANGELOG.md)
[![License](https://img.shields.io/badge/license-AGPL--3.0-blue)](./LICENSE)

## What It Is

| Dimension | Answer |
|-----------|--------|
| **Purpose** | Multi-tenant LLM gateway + admin console. Rust backend + Svelte frontend, single 17 MB binary |
| **Core use case** | Issue API keys per team, route to N upstream providers, settle to monthly invoices, enforce quotas, full audit trail |
| **What it's not** | Not an LLM routing SDK (use LiteLLM). Not a stateless proxy (use Cloudflare AI Gateway). Not a single-tenant chat UI (use LobeChat) |

## How It Compares

| | Kooix Gate | LiteLLM | OneAPI / NewAPI | OpenRouter |
|---|---|---|---|---|
| **Multi-org tenancy** | 3-layer + RLS fallback | Single tenant | User-level | SaaS |
| **Streaming billing** | Fail-closed + outbox | Gaps possible | Silent under-count | SaaS |
| **Custom provider** | JSON manifest (5 min) | Python config | Modify Go source | No |
| **WASM Plugin** | Transform hooks (ADR-0003 v0) | No | No | No |
| **Quota dimensions** | rpm/tpm/concurrent/budget/lifetime + dry-run | rpm/tpm | rpm/tpm | quota |
| **Runtime** | Rust + compile-time SQL | Python | Go | Closed source |
| **Binary size** | 17 MB | ~500 MB image | ~30 MB | n/a |
| **Provider presets** | 55 built-in | 100+ (Python config) | ~30 | n/a |

> **TL;DR:** Kooix Gate brings OneAPI's product shape done right in Rust, with LiteLLM's onboarding convenience via declarative manifests.

## 30-Second Start

```bash
docker compose up -d                      # PG + Redis + migrations + API + Web
open http://localhost:8080                # Admin console (SvelteKit)
# API is at http://localhost:8000
```

> UI runs on `:8080`, API on `:8000`. Use nginx/caddy to unify in production. CORS permissive mode is on by default.

For building from source, see [Manual Setup](#manual-setup).

## Documentation Map

Getting started / Deployment:

- [docs/getting-started.md](./docs/getting-started.md) — Three paths: 30s Docker / 5min Helm / 10min source
- [RELEASE.md](./RELEASE.md) — Release, rollback, and smoke test runbook

Architecture / Design:

- [DESIGN.md](./DESIGN.md) — Domain model, runtime boundaries, data flow
- [docs/architecture.md](./docs/architecture.md) — C4 overview (control / data / worker plane)
- [docs/architecture/decisions/](./docs/architecture/decisions/) — ADRs (ADR-0001 through ADR-0005, all Accepted/Implemented)

Extension:

- [docs/plugin-manifest.md](./docs/plugin-manifest.md) — HTTP Plugin manifest spec and examples
- [docs/wasm-plugin-abi.md](./docs/wasm-plugin-abi.md) — WASM Plugin ABI v0 full design
- [docs/wasm-sdk-as.md](./docs/wasm-sdk-as.md) — AssemblyScript SDK guide
- [docs/manifest-registry-signature.md](./docs/manifest-registry-signature.md) — Registry signature schema

API / Integration:

- [docs/api-reference.md](./docs/api-reference.md) — OpenAPI / Postman / Bruno + key API index
- [examples/](./examples/) — SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm

Observability / Ops:

- [docs/observability.md](./docs/observability.md) — Prometheus / Grafana / OTLP
- [docs/observability-runbook.md](./docs/observability-runbook.md) — SLO metrics / incident playbook
- [docs/security-runbook.md](./docs/security-runbook.md) — Key rotation / master key recovery
- [docs/wasm-runbook.md](./docs/wasm-runbook.md) — WASM module troubleshooting
- [docs/threat-model.md](./docs/threat-model.md) — Threat model

Roadmap / Gaps:

- [ROADMAP.md](./ROADMAP.md) — Four milestones (M1/M2/M3 shipped, M4 candidate)
- [docs/product-gaps.md](./docs/product-gaps.md) — v0.5.0 product gap tracker

## Core Capabilities

| Layer | Capabilities |
|-------|-------------|
| Tenancy | Org x Project x ApiKey — 3-layer RBAC + Postgres RLS fallback |
| Gateway | OpenAI-compatible chat/embeddings/images/audio/responses, streaming SSE + tool calling |
| Providers | HTTP Plugin manifest v1 + 55 built-in presets (OpenAI, Anthropic, Azure, Bedrock, Gemini, xAI, DeepSeek, Mistral, Groq, Fireworks, Nvidia NIM, DeepInfra, Perplexity, SiliconFlow, etc.) |
| Routing | priority / weighted_random / round_robin / least_conn / least_latency + fallback + canary |
| Billing | Multi-dimensional pricing + crash-safe pre-debit + ledger + invoice state machine |
| Quotas | rpm / tpm / concurrent / daily / monthly / lifetime + dry-run / explain |
| Identity | Argon2id + JWT + API Key SHA-256 + OIDC SSO + refresh session rotation |
| Console | SvelteKit admin UI + Playground workflow editor |
| Ops | `kgctl` CLI + Docker Compose + Helm + Prometheus + OpenTelemetry |
| Extensibility | WASM transform plugins (ADR-0003 v0) with Rust + AssemblyScript SDKs |

## Supported Providers (55)

**Major Providers:**
OpenAI, Anthropic, Azure OpenAI, Google Vertex AI, Google Gemini, AWS Bedrock, xAI (Grok), DeepSeek, Mistral, Cohere, AI21 Labs

**Inference Platforms:**
Groq, Together AI, Fireworks AI, OpenRouter, Perplexity, Cerebras, SambaNova, DeepInfra, Nvidia NIM, Replicate, Novita AI, Lambda, Lepton AI, Nebius AI, Friendli AI, Chutes AI, Hyperbolic, Cloudflare AI

**Embedding / Specialty:**
Jina AI, Voyage AI

**China Providers:**
Moonshot, Zhipu GLM, Qwen (Tongyi), Yi (Lingyiwanwu), Baichuan, MiniMax, Stepfun, SiliconFlow, Doubao (Volcengine), Hunyuan (Tencent), Spark (iFlytek), Infini AI

**Self-hosted / Local:**
Ollama, vLLM, LM Studio, LocalAI, Xinference, Text Generation Inference (TGI), Jan, Llamafile, GPT4All, TabbyAPI

**Generic:** OpenAI-compatible (any endpoint following the OpenAI API format)

**Native providers (procedural, for non-standard protocols):** Codex, Kiro, Windsurf

> Any OpenAI-compatible endpoint can be added via a JSON manifest without modifying code.

## Tech Stack

| Layer | Choice |
|-------|--------|
| Backend | Rust 2024 · Axum 0.7 · Tokio · sqlx 0.8 (compile-time checked) |
| Storage | PostgreSQL 15+ (optional TimescaleDB) · Redis (fred) |
| Frontend | SvelteKit 2 · Svelte 5 · TypeScript · Tailwind v4 · @xyflow/svelte |
| Auth | Argon2id + JWT (HS256) + API Key (SHA-256) + OIDC |
| Crypto | AES-256-GCM envelope encryption + KMS abstraction |
| Observability | tracing + OpenTelemetry + Prometheus |

## Workspace Structure

```
kooix-gate/
├── Cargo.toml                  # workspace
├── crates/
│   ├── gate-core/              # Domain types (typed IDs / Identity / RBAC / Quota)
│   ├── gate-crypto/            # Envelope encryption + KMS abstraction
│   ├── gate-storage/           # PostgreSQL repository (Pg + InMemory)
│   │   └── migrations/         # 37 SQL files with RLS / pricing / retention policies
│   ├── gate-auth/              # Password / JWT / API Key / OIDC / AuthContext
│   ├── gate-cache/             # Redis Lua (rate limit + quota)
│   ├── gate-providers/         # Provider trait + adapters + ProviderRouter + WASM integration
│   ├── gate-billing/           # Outbox + multi-dimensional pricing + LiteLLM sync
│   ├── gate-wasm/              # WASM runtime (wasmtime 26 + 3 hooks + fallback + Prometheus)
│   ├── gate-wasm-sdk/          # Rust SDK for writing WASM transform plugins
│   ├── gate-server/            # Axum HTTP gateway (main binary)
│   └── kgctl/                  # Deployment & ops CLI
├── sdks/
│   └── gate-wasm-sdk-as/       # AssemblyScript SDK (@kooix-gate/wasm-sdk-as)
├── deploy/
│   ├── helm/gate/              # Helm chart (values + templates)
│   └── grafana/dashboards/     # Grafana dashboard JSON
├── bench/                      # Load testing (K6) + mock upstream
├── examples/                   # SDK / curl / Postman / Bruno / OpenAPI / Terraform / Helm
├── docs/                       # Architecture / runbooks / ADRs / product gaps
└── web/                        # SvelteKit admin console
```

## Quick Start (Docker)

```bash
# Full deployment (build image + PG / Redis / migrations / server)
docker compose up -d

# Infrastructure only (local dev — compile and run backend yourself)
docker compose -f docker-compose.dev.yml up -d
cargo run -p gate-server
```

Visit `http://localhost:8000` after startup.

> **Production:** Replace `KOOIX_JWT_SECRET`, `KOOIX_MASTER_KEY`, and `POSTGRES_PASSWORD` in `docker-compose.yml`.
> Use `kgctl init` to generate secure keys.
> For JWT rotation: put the new key in `KOOIX_JWT_SECRET`, move the old one to `KOOIX_JWT_PREVIOUS_SECRETS` (comma-separated, verify-only window).

## Manual Setup

### 1. Start Dependencies

```bash
docker run -d --name kg-pg -e POSTGRES_PASSWORD=devpass \
  -e POSTGRES_DB=kooix_gate -p 5432:5432 postgres:17-alpine
docker run -d --name kg-redis -p 6379:6379 redis:7-alpine
```

### 2. Generate Keys + Run Migrations

```bash
cargo install --path crates/kgctl

# Generate master key + JWT secret
kgctl init > .env
source .env

export KOOIX_DATABASE_URL=postgres://postgres:devpass@localhost/kooix_gate
export KOOIX_REDIS_URL=redis://localhost:6379/0
export KOOIX_PUBLIC_URL=http://localhost:8080
# Optional: decrypted channel key short-cache TTL; 0 disables, default 30s
export KOOIX_CHANNEL_KEY_CACHE_TTL_SECS=30

kgctl migrate
kgctl doctor    # Validates env / JWT rotation / migrations / Redis Lua
kgctl seed-pricing
kgctl admin create --email you@example.com
```

### 3. Start Services

```bash
# Backend
cargo run -p gate-server --release

# Console
cd web && npm install && npm run dev
```

### 4. Make a Chat Request

```bash
# Log in as admin, create an API key from the console
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer sk-kg-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"hi"}]}'
```

## Design Highlights

- **Multi-org 3-layer tenancy:** Org -> Project -> ApiKey, never coupled
- **Composite membership key** `(OrgId, ProjectId)`: prevents cross-org replay
- **RLS fallback:** even if application-layer filtering leaks, tenant data stays isolated
- **Fail-closed streaming billing:** `stream_options.include_usage` force-injected; final-frame usage captured via outbox pattern; worker settles to `usage_records` + `billing_ledger_events`
- **HTTP Plugin normalization:** manifest declares request/response/SSE paths; everything normalizes to OpenAI-compatible `ChatResponse` / `ChatStreamChunk` / `EmbeddingResponse`; manifests treated as untrusted config with template variable sandboxing, outbound allowlist, DNS rebind guard, header redaction, and size limits
- **Strongly typed IDs:** compile-time prevention of `OrgId` / `UserId` mix-ups
- **AuthContext single permission facade:** no raw role map reads — everything goes through `can()` / `require!`
- **Crash-safe pre-debit:** budget quota pre-debited in Redis; `quota_keys` + `estimated_micros` written to `inflight_requests`; normal settle adjusts, drop refunds, process crash handled by sweeper

Full architecture in [DESIGN.md](./DESIGN.md). Plugin manifest examples in [docs/plugin-manifest.md](./docs/plugin-manifest.md). Documentation index at [docs/README.md](./docs/README.md).

## Testing

```bash
# Full suite (556+ Rust + 127 web tests, includes testcontainers integration tests — needs Docker)
cargo test --workspace

# Quick unit tests only (no Docker)
cargo test --workspace --lib

# Skip PG integration tests
KOOIX_SKIP_PG_TESTS=1 cargo test --workspace

# Lint gates
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Console build:

```bash
cd web && npm run check && npm test && npm run build
```

## License

[AGPL-3.0-only](./LICENSE)
