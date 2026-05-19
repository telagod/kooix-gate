#!/usr/bin/env bash
set -euo pipefail

: "${KOOIX_BASE_URL:=http://localhost:8000}"
: "${KOOIX_ADMIN_TOKEN:?KOOIX_ADMIN_TOKEN is required}"
: "${KOOIX_ORG_ID:?KOOIX_ORG_ID is required}"
: "${SCOPE_KIND:=project}"
: "${SCOPE_ID:?SCOPE_ID is required, e.g. proj_... / org_... / key_...}"
: "${DIMENSION:=rpm}"
: "${LIMIT_VALUE:=60}"
: "${WINDOW_SECONDS:=60}"
: "${MODEL_FILTER:=}"


raw_id() {
  local value="$1"
  if [[ "$value" =~ ^[a-z]+_[0-9a-f]{32}$ ]]; then
    local hex="${value##*_}"
    printf '%s-%s-%s-%s-%s' "${hex:0:8}" "${hex:8:4}" "${hex:12:4}" "${hex:16:4}" "${hex:20:12}"
  else
    printf '%s' "$value"
  fi
}

jq -nc \
  --arg scope_kind "$SCOPE_KIND" \
  --arg scope_id "$(raw_id "$SCOPE_ID")" \
  --arg dimension "$DIMENSION" \
  --arg limit_value "$LIMIT_VALUE" \
  --argjson window_seconds "$WINDOW_SECONDS" \
  --arg model_filter "$MODEL_FILTER" \
  '{
    scope_kind: $scope_kind,
    scope_id: $scope_id,
    dimension: $dimension,
    model_filter: (if $model_filter == "" then null else $model_filter end),
    limit_value: $limit_value,
    window_seconds: $window_seconds
  }' | curl -fsS -X POST "${KOOIX_BASE_URL%/}/v1/orgs/$(raw_id "$KOOIX_ORG_ID")/quotas" \
    -H "Authorization: Bearer ${KOOIX_ADMIN_TOKEN}" \
    -H "X-Kooix-Org: ${KOOIX_ORG_ID}" \
    -H "Content-Type: application/json" \
    -d @- | jq .
