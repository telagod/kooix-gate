# Private auth + field-map SSE

Manifest package `private-auth-field-map-sse` demonstrates the required P1.8 package directory layout:

- `manifest.json` — HTTP Plugin manifest consumed by channel `model_mapping.plugin`.
- `fixtures/private-auth-field-map-sse.fixture.json` — request/response/SSE replay fixture for regression tests.
- `README.md` — integration notes for operators.
- `security.md` — risk statement and review checklist.

## Integration

1. Review `security.md` before importing.
2. Create a plugin channel with `manifest.json`.
3. Store the upstream credential in a channel key labeled `primary` or `api_key`.
4. Run `kgctl plugin package lint examples/manifest-packages/private-auth-field-map-sse --verify` before publishing.

The sample upstream expects `POST /private/chat/{{model}}`, an `X-Api-Key` header, a non-stream response shaped as `result.*`, and SSE frames under `payload.*`.
