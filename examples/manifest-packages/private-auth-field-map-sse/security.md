# Security

Risk statement for manifest package `private-auth-field-map-sse`.

## Secret handling

- No API key, bearer token, password, or other secret is stored in `manifest.json`, fixtures, README, or this file.
- Runtime credentials must come from Kooix Gate `channel_keys` secret slots (`primary` / `api_key`) or environment fallback in local development.
- Review fixture payloads before publishing; fixtures must use synthetic request IDs and sample text only.

## Network boundary

- `allow_absolute_chat_path` is `false`; the package only declares a relative `/private/chat/{{model}}` path.
- `outbound_allowlist` is empty in the portable sample, so runtime denylist/DNS-rebind guard still applies while operators can set an environment-specific origin allowlist at import time.
- Operators should still enforce egress allowlists/firewall policy around the configured upstream Base URL.

## Size and parsing limits

- `max_request_bytes`, `max_response_bytes`, and `max_sse_event_bytes` are explicitly bounded; `request.timeout_ms` can be set per deployment if the upstream SLA is lower than channel timeout.
- The SSE mapping only consumes `payload.*` fields and treats `EOF` as a done sentinel.
