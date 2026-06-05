# ADR-0006: WASM ABI v1 — Component Model + wit-bindgen

- Status: **Proposed (2026-06-04)**
- Deciders: telagod
- Affected: `crates/gate-wasm/`, `crates/gate-wasm-sdk/`, `sdks/gate-wasm-sdk-as/`, `crates/gate-providers/src/custom_provider/`
- Supersedes: ADR-0003 v0 ABI (kept as compatibility baseline during dual-run window)
- Related: [G-103](../../product-gaps.md#g-103-wasm-abi-v1-走-wit-bindgen), [wasm-plugin-abi.md](../../wasm-plugin-abi.md)

## Context

ADR-0003 v0 ABI uses hand-written i32/i64 calling conventions (`gate_alloc` + packed pointer return). This works but:

1. **Cross-language SDK maintenance is O(N):** every new guest language needs its own alloc/pack/unpack boilerplate.
2. **Type safety is manual:** host and guest agree on JSON envelope by convention; ABI-level mismatch is silent corruption.
3. **No structured error returns:** v0 hooks return raw bytes; errors are side-band (fuel exhaustion) or in-band (JSON error field).
4. **Industry direction:** WASI component model is stabilized; wasmtime 45+ has production-grade support; wit-bindgen generates host + guest bindings for Rust/Go/Python/JS.

## Decision

Migrate to **WASI Component Model** with `wit-bindgen` for both host (gate-wasm) and guest (gate-wasm-sdk) bindings.

### WIT Definition

`crates/gate-wasm/wit/kooix-plugin.wit` defines package `kooix:plugin@0.1.0`:

- **`host` interface:** `get-secret`, `log`, `record-metric`, `now-ms`, `nonce`, `redact`
- **`types` interface:** `transform-input`, `transform-output`, `transform-error`
- **`transform` interface (guest export):** `transform-request`, `transform-response`, `transform-stream-event`, `finish-stream`
- **`plugin` world:** imports host + types, exports transform

### Migration Strategy

**Dual-run window (v0.5.x):**

| Phase | Content | Breaking |
|-------|---------|----------|
| P1 — Foundation | Upgrade wasmtime 26 → 45; add `component-model` feature; WIT file committed | No |
| P2 — Host bindings | Generate host-side bindings via `wasmtime::component::bindgen!`; implement `host` interface; wire into `WasmtimeHost` | No (v0 still works) |
| P3 — Guest SDK | New `gate-wasm-sdk-v1` with `wit-bindgen::generate!`; AS SDK v1 via `jco` | No (v0 SDK still works) |
| P4 — Runtime detection | `WasmtimeHost::load_module` detects core module (v0) vs component (v1) and dispatches accordingly | No |
| P5 — Deprecation | v0 ABI deprecated; v1 is default; one minor version grace period | Yes (announcement) |
| P6 — Removal | v0 ABI code removed | Yes |

### Wasmtime Upgrade Path

wasmtime 26 → 45:

- `Config::wasm_component_model(true)` enables component model
- `wasmtime::component::{Component, Linker, Instance}` replaces raw module API for v1
- `consume_fuel` and async support remain compatible
- cwasm cache format changes between major versions (already handled by sha256+version naming)

### Key Design Decisions

1. **JSON stays as string, not typed records:** the WIT `transform-input.json` field is a `string` (JSON text), not a typed record. This avoids freezing the OpenAI request/response schema into the ABI. Provider-specific fields evolve without ABI version bumps. Performance cost (~1 JSON parse per hook) is acceptable at 50ms budget.

2. **Optional exports via component model:** component model allows partial interface export. Unimplemented hooks are identity passthrough (same as v0).

3. **Host function additions:** v1 adds `now-ms`, `nonce`, `redact` over v0's `host_log` + `host_record_metric` + `host_get_secret_slot`. These enable deterministic-but-timestamped transforms without breaking sandbox guarantees.

4. **Error typing:** `transform-error` variant replaces v0's in-band JSON errors, giving the host structured error handling (retry decisions, audit classification, metric labeling).

## Consequences

### Positive

- **N guest languages from 1 WIT:** wit-bindgen generates Rust, Go, Python, JS, C guest bindings. AS SDK generation via `jco transpile`.
- **Type-safe ABI boundary:** mismatched types fail at component instantiation, not at runtime.
- **Structured errors:** host can make retry/fallback/audit decisions without parsing JSON error fields.
- **Ecosystem alignment:** plugins built for other component-model hosts need minimal adaptation.

### Negative

- **wasmtime upgrade risk:** 26 → 45 is a large jump; API surface changes need validation.
- **Dual-run complexity:** maintaining v0 + v1 dispatch for one minor version.
- **Component overhead:** component model adds a thin virtualization layer (~50μs per call); acceptable within 50ms budget.
- **cwasm cache invalidation:** all existing cwasm files invalidated by wasmtime version bump.

### Risks

| Risk | Mitigation |
|------|-----------|
| wasmtime 45 API breakage | Pin exact minor; integration tests catch regressions |
| Component model perf regression | Benchmark v0 vs v1 in `gate-wasm/benches/` before P5 |
| Guest SDK migration friction | Provide `examples/wasm-transform-v1/` + migration guide |
| AS SDK component support | Verify `jco componentize` works for AS output before P3 |

## Implementation Checklist

- [x] WIT definition: `crates/gate-wasm/wit/kooix-plugin.wit`
- [x] P1: wasmtime 26 → 45 upgrade + `component-model` feature
- [x] P2: host-side `bindgen!` + host interface impl (6 host fns)
- [x] P3: guest SDK v1 (Rust wit-bindgen) + example plugin
- [x] P4: v0/v1 runtime detection + `UnifiedWasmHost` dispatch
- [x] P5: v0 `#[deprecated]` on `WasmtimeHost`, docs updated
- [ ] P6: v0 removal (planned v0.6.0)

## References

- [WIT specification](https://component-model.bytecodealliance.org/design/wit.html)
- [wasmtime component model docs](https://docs.wasmtime.dev/api/wasmtime/component/index.html)
- [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen)
- [ADR-0003 v0 ABI](./ADR-0003-wasm-plugin-abi-v0.md)
- [G-103 product gap](../../product-gaps.md#g-103-wasm-abi-v1-走-wit-bindgen)
