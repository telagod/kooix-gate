#!/usr/bin/env bash
set -euo pipefail

: "${KOOIX_BASE_URL:=http://localhost:8000}"
: "${KOOIX_ADMIN_TOKEN:?KOOIX_ADMIN_TOKEN is required}"
: "${KOOIX_PROJECT_ID:?KOOIX_PROJECT_ID is required}"
: "${UPSTREAM_BASE_URL:?UPSTREAM_BASE_URL is required}"
: "${UPSTREAM_API_KEY:?UPSTREAM_API_KEY is required}"
: "${MODEL:=gpt-4o-mini}"
: "${CHANNEL_CODE:=openai-compatible-main}"
: "${GROUP_NAME:=Default OpenAI Compatible}"


raw_id() {
  local value="$1"
  if [[ "$value" =~ ^[a-z]+_[0-9a-f]{32}$ ]]; then
    local hex="${value##*_}"
    printf '%s-%s-%s-%s-%s' "${hex:0:8}" "${hex:8:4}" "${hex:12:4}" "${hex:16:4}" "${hex:20:12}"
  else
    printf '%s' "$value"
  fi
}

api() {
  local method="$1" path="$2" body="${3:-}"
  if [[ -n "$body" ]]; then
    curl -fsS -X "$method" "${KOOIX_BASE_URL%/}${path}" \
      -H "Authorization: Bearer ${KOOIX_ADMIN_TOKEN}" \
      -H "Content-Type: application/json" \
      -d "$body"
  else
    curl -fsS -X "$method" "${KOOIX_BASE_URL%/}${path}" \
      -H "Authorization: Bearer ${KOOIX_ADMIN_TOKEN}"
  fi
}

channel_json="$(jq -nc \
  --arg code "$CHANNEL_CODE" \
  --arg base_url "$UPSTREAM_BASE_URL" \
  --arg model "$MODEL" \
  --slurpfile manifest "$(dirname "$0")/../manifests/openai-compatible.json" \
  '{
    code: $code,
    name: $code,
    provider_type: "plugin",
    base_url: $base_url,
    enabled: true,
    supported_models: [$model],
    tags: ["example", "openai-compatible"],
    timeout_ms: 30000,
    max_retries: 1,
    model_mapping: $manifest[0]
  }')"

channel_id="$(api POST /v1/admin/channels "$channel_json" | jq -r '.id')"
printf 'channel_id=%s\n' "$channel_id"

api POST "/v1/admin/channels/$(raw_id "$channel_id")/keys" \
  "$(jq -nc --arg secret "$UPSTREAM_API_KEY" '{secret:$secret, alias:"primary"}')" >/dev/null

group_id="$(api POST /v1/admin/groups \
  "$(jq -nc --arg name "$GROUP_NAME" '{name:$name, strategy:"priority"}')" | jq -r '.id')"
printf 'group_id=%s\n' "$group_id"

api POST "/v1/admin/groups/$(raw_id "$group_id")/bindings" \
  "$(jq -nc --arg channel_id "$(raw_id "$channel_id")" '{channel_id:$channel_id, priority:1, weight:1}')" >/dev/null

api PUT "/v1/admin/projects/$(raw_id "$KOOIX_PROJECT_ID")/default-group" \
  "$(jq -nc --arg group_id "$(raw_id "$group_id")" '{group_id:$group_id}')" >/dev/null

printf 'default route bound: project=%s group=%s channel=%s\n' "$KOOIX_PROJECT_ID" "$group_id" "$channel_id"
