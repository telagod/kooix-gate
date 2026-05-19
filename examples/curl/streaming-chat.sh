#!/usr/bin/env bash
set -euo pipefail

: "${KOOIX_BASE_URL:=http://localhost:8000}"
: "${MODEL:=gpt-4o-mini}"
: "${KOOIX_API_KEY:?KOOIX_API_KEY is required}"

curl -N "${KOOIX_BASE_URL%/}/v1/chat/completions" \
  -H "Authorization: Bearer ${KOOIX_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "$(jq -nc --arg model "$MODEL" '{
    model: $model,
    stream: true,
    messages: [{role:"user", content:"Stream three short tokens."}]
  }')"
