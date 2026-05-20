#!/usr/bin/env bash
set -euo pipefail

# Kooix Gate end-to-end demo.
#
# Flow:
#   1. docker compose up (PostgreSQL / Redis / gate-server)
#   2. first-time setup or existing admin login
#   3. create OpenAI-compatible provider preset channel
#   4. create Project API key
#   5. send one chat completion
#   6. read usage and monthly billing
#
# Required for a live provider demo:
#   export UPSTREAM_BASE_URL="https://api.openai.com/v1"
#   export UPSTREAM_API_KEY="<provider-key>"
#
# Optional:
#   export KOOIX_BASE_URL="http://localhost:8000"
#   export KOOIX_ADMIN_EMAIL="root@example.com"
#   export KOOIX_ADMIN_PASSWORD="change-me-demo-password"
#   export MODEL="gpt-4o-mini"

: "${KOOIX_BASE_URL:=http://localhost:8000}"
: "${KOOIX_ADMIN_EMAIL:=root@example.com}"
: "${KOOIX_ADMIN_PASSWORD:=change-me-demo-password}"
: "${KOOIX_ORG_NAME:=Demo Org}"
: "${KOOIX_ORG_SLUG:=demo}"
: "${KOOIX_PROJECT_NAME:=Demo Project}"
: "${KOOIX_PROJECT_SLUG:=demo}"
: "${MODEL:=gpt-4o-mini}"
: "${CHANNEL_CODE:=demo-openai-compatible}"
: "${GROUP_NAME:=Demo OpenAI Compatible}"
: "${BILLING_MONTH:=$(date +%Y-%m)}"
: "${COMPOSE:=docker compose}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing dependency: $1" >&2
    exit 1
  }
}

raw_id() {
  local value="$1"
  if [[ "$value" =~ ^[a-z]+_[0-9a-f]{32}$ ]]; then
    local hex="${value##*_}"
    printf '%s-%s-%s-%s-%s' "${hex:0:8}" "${hex:8:4}" "${hex:12:4}" "${hex:16:4}" "${hex:20:12}"
  else
    printf '%s' "$value"
  fi
}

api_json() {
  local method="$1" path="$2" token="${3:-}" body="${4:-}"
  local args=(-fsS -X "$method" "${KOOIX_BASE_URL%/}${path}" -H "Content-Type: application/json")
  if [[ -n "$token" ]]; then
    args+=(-H "Authorization: Bearer ${token}")
  fi
  if [[ -n "$body" ]]; then
    args+=(-d "$body")
  fi
  curl "${args[@]}"
}

api_json_with_org() {
  local method="$1" path="$2" token="$3" org_id="$4" body="${5:-}"
  local args=(-fsS -X "$method" "${KOOIX_BASE_URL%/}${path}" -H "Authorization: Bearer ${token}" -H "X-Kooix-Org: ${org_id}" -H "Content-Type: application/json")
  if [[ -n "$body" ]]; then
    args+=(-d "$body")
  fi
  curl "${args[@]}"
}

wait_health() {
  for _ in {1..90}; do
    if curl -fsS "${KOOIX_BASE_URL%/}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  echo "gate-server did not become healthy at ${KOOIX_BASE_URL}" >&2
  $COMPOSE logs --tail=120 server >&2 || true
  exit 1
}

login() {
  api_json POST /v1/auth/login '' "$(jq -nc --arg email "$KOOIX_ADMIN_EMAIL" --arg password "$KOOIX_ADMIN_PASSWORD" '{email:$email,password:$password}')" | jq -r '.access_token'
}

setup_or_login() {
  local status initialized token
  status="$(curl -fsS "${KOOIX_BASE_URL%/}/health/status")"
  initialized="$(jq -r '.initialized' <<<"$status")"
  if [[ "$initialized" == "false" ]]; then
    echo "==> First-time setup: ${KOOIX_ADMIN_EMAIL}" >&2
    api_json POST /v1/setup '' "$(jq -nc \
      --arg email "$KOOIX_ADMIN_EMAIL" \
      --arg password "$KOOIX_ADMIN_PASSWORD" \
      --arg org_name "$KOOIX_ORG_NAME" \
      --arg org_slug "$KOOIX_ORG_SLUG" \
      --arg project_name "$KOOIX_PROJECT_NAME" \
      --arg project_slug "$KOOIX_PROJECT_SLUG" \
      '{email:$email,password:$password,org_name:$org_name,org_slug:$org_slug,project_name:$project_name,project_slug:$project_slug}')" >/tmp/kooix-demo-setup.json
    jq '{org_id, project_id, email}' /tmp/kooix-demo-setup.json >&2
  else
    echo "==> Existing setup detected; using admin login" >&2
  fi

  token="$(login)"
  if [[ -z "$token" || "$token" == "null" ]]; then
    echo "login failed: empty access token" >&2
    exit 1
  fi
  printf '%s' "$token"
}

ensure_org_project() {
  local token="$1" me org_id project_id
  me="$(api_json GET /v1/me "$token")"
  org_id="$(jq -r '.orgs[0] // empty' <<<"$me")"
  if [[ -z "$org_id" ]]; then
    org_id="$(api_json POST /v1/admin/orgs "$token" "$(jq -nc --arg name "$KOOIX_ORG_NAME" --arg slug "${KOOIX_ORG_SLUG}-$(date +%s)" '{name:$name,slug:$slug}')" | jq -r '.id')"
  fi

  project_id="$(api_json_with_org GET "/v1/orgs/$(raw_id "$org_id")/projects" "$token" "$org_id" | jq -r '.[0].id // empty')"
  if [[ -z "$project_id" ]]; then
    project_id="$(api_json_with_org POST "/v1/orgs/$(raw_id "$org_id")/projects" "$token" "$org_id" "$(jq -nc --arg name "$KOOIX_PROJECT_NAME" --arg slug "${KOOIX_PROJECT_SLUG}-$(date +%s)" '{name:$name,slug:$slug}')" | jq -r '.id')"
  fi

  jq -nc --arg org_id "$org_id" --arg project_id "$project_id" '{org_id:$org_id,project_id:$project_id}'
}

create_channel_route() {
  local token="$1" project_id="$2"
  : "${UPSTREAM_BASE_URL:?UPSTREAM_BASE_URL is required for provider channel demo}"
  : "${UPSTREAM_API_KEY:?UPSTREAM_API_KEY is required for provider channel demo}"

  KOOIX_ADMIN_TOKEN="$token" \
  KOOIX_PROJECT_ID="$project_id" \
  KOOIX_BASE_URL="$KOOIX_BASE_URL" \
  UPSTREAM_BASE_URL="$UPSTREAM_BASE_URL" \
  UPSTREAM_API_KEY="$UPSTREAM_API_KEY" \
  MODEL="$MODEL" \
  CHANNEL_CODE="${CHANNEL_CODE}-$(date +%s)" \
  GROUP_NAME="${GROUP_NAME} $(date +%H%M%S)" \
    examples/admin/create-provider-preset-channel.sh
}

create_pricing_rules() {
  local token="$1"
  echo "==> Creating pricing rules for ${MODEL}"
  KOOIX_ADMIN_TOKEN="$token" KOOIX_BASE_URL="$KOOIX_BASE_URL" MODEL="$MODEL" DIMENSION=input_tokens RATE=0.15 \
    examples/admin/create-pricing-rule.sh >/tmp/kooix-demo-pricing-input.json
  KOOIX_ADMIN_TOKEN="$token" KOOIX_BASE_URL="$KOOIX_BASE_URL" MODEL="$MODEL" DIMENSION=output_tokens RATE=0.60 \
    examples/admin/create-pricing-rule.sh >/tmp/kooix-demo-pricing-output.json
  jq -s '[.[].id]' /tmp/kooix-demo-pricing-input.json /tmp/kooix-demo-pricing-output.json
}

create_api_key() {
  local token="$1" org_id="$2" project_id="$3"
  api_json_with_org POST "/v1/orgs/$(raw_id "$org_id")/projects/$(raw_id "$project_id")/api-keys" "$token" "$org_id" \
    "$(jq -nc --arg model "$MODEL" '{name:"demo-key", allowed_models:[$model]}')" | jq -r '.plaintext'
}

send_chat() {
  local api_key="$1"
  api_json POST /v1/chat/completions "$api_key" "$(jq -nc --arg model "$MODEL" '{model:$model,messages:[{role:"user",content:"Reply with exactly: Kooix demo ok"}]}')" | jq '{id, model, usage, text: .choices[0].message.content}'
}

show_usage_and_billing() {
  local token="$1" org_id="$2"
  echo "==> Usage"
  api_json_with_org GET "/v1/usage?range=7d&group_by=day" "$token" "$org_id" | jq '{total_cost_usd,total_tokens_in,total_tokens_out,series_len:(.series|length)}'
  echo "==> Billing ${BILLING_MONTH}"
  api_json_with_org GET "/v1/orgs/$(raw_id "$org_id")/billing/${BILLING_MONTH}" "$token" "$org_id" | jq '{month,total_cost_usd,total_requests,total_tokens_in,total_tokens_out,invoice:.invoice.status}'
}

need docker
need curl
need jq

echo "==> docker compose up -d --build"
$COMPOSE up -d --build
wait_health

echo "==> Login / setup"
admin_token="$(setup_or_login)"
org_project="$(ensure_org_project "$admin_token")"
org_id="$(jq -r '.org_id' <<<"$org_project")"
project_id="$(jq -r '.project_id' <<<"$org_project")"
echo "org_id=${org_id}"
echo "project_id=${project_id}"

echo "==> Provider preset channel"
create_channel_route "$admin_token" "$project_id"
create_pricing_rules "$admin_token"

echo "==> Project API key"
api_key="$(create_api_key "$admin_token" "$org_id" "$project_id")"
echo "api_key=${api_key:0:12}…"

echo "==> Chat"
send_chat "$api_key"
# Give the async billing worker a small window to settle outbox.
sleep 3
show_usage_and_billing "$admin_token" "$org_id"

echo "demo ok"
