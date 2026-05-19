#!/usr/bin/env bash
set -euo pipefail

: "${KOOIX_BASE_URL:=http://localhost:8000}"
: "${KOOIX_ADMIN_TOKEN:?KOOIX_ADMIN_TOKEN is required}"
: "${MODEL:=gpt-4o-mini}"
: "${DIMENSION:=input_tokens}"
: "${UNIT:=per_million_tokens}"
: "${RATE:=0.15}"
: "${PRIORITY:=100}"

jq -nc \
  --arg model "$MODEL" \
  --arg dimension "$DIMENSION" \
  --arg unit "$UNIT" \
  --argjson rate "$RATE" \
  --argjson priority "$PRIORITY" \
  '{
    model: $model,
    dimension: $dimension,
    unit: $unit,
    rate: $rate,
    conditions: {},
    priority: $priority,
    description: "example pricing rule"
  }' | curl -fsS -X POST "${KOOIX_BASE_URL%/}/v1/admin/pricing-rules" \
    -H "Authorization: Bearer ${KOOIX_ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    -d @- | jq .
