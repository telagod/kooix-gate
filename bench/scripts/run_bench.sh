#!/usr/bin/env bash
# ===========================================================================
# run_bench.sh — Orchestrates kooix-gate load testing
#
# Usage:
#   ./bench/scripts/run_bench.sh                    # defaults
#   API_URL=http://gate:8080 ./bench/scripts/run_bench.sh
#   MOCK_LATENCY=100 ./bench/scripts/run_bench.sh   # 100ms upstream
#   SKIP_MOCK=1 ./bench/scripts/run_bench.sh        # use external upstream
# ===========================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Configuration
API_URL="${API_URL:-http://localhost:8080}"
API_KEY="${API_KEY:-sk-kg-test-key}"
MOCK_PORT="${MOCK_PORT:-9999}"
MOCK_LATENCY="${MOCK_LATENCY:-50}"
SKIP_MOCK="${SKIP_MOCK:-}"
RESULTS_DIR="${ROOT_DIR}/bench/results"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"

# Ensure results directory exists
mkdir -p "${RESULTS_DIR}"

# Cleanup handler
PIDS_TO_KILL=()
cleanup() {
    echo ""
    echo "=== Cleaning up ==="
    for pid in "${PIDS_TO_KILL[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            echo "  Stopping PID $pid"
            kill "$pid" 2>/dev/null || true
            wait "$pid" 2>/dev/null || true
        fi
    done
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

echo "=== kooix-gate load test ==="
echo "  Timestamp:    ${TIMESTAMP}"
echo "  API URL:      ${API_URL}"
echo "  Mock latency: ${MOCK_LATENCY}ms"
echo ""

# Check k6 is available
if ! command -v k6 &>/dev/null; then
    echo "ERROR: k6 is not installed."
    echo "  Install: https://grafana.com/docs/k6/latest/set-up/install-k6/"
    echo "  macOS:   brew install k6"
    echo "  Linux:   snap install k6"
    exit 1
fi

# Check python3 for mock (unless skipped)
if [[ -z "${SKIP_MOCK}" ]] && ! command -v python3 &>/dev/null; then
    echo "ERROR: python3 is not installed (needed for mock upstream)."
    exit 1
fi

# ---------------------------------------------------------------------------
# Start mock upstream
# ---------------------------------------------------------------------------

if [[ -z "${SKIP_MOCK}" ]]; then
    echo "=== Starting mock LLM upstream (port=${MOCK_PORT}, latency=${MOCK_LATENCY}ms) ==="
    python3 "${ROOT_DIR}/bench/scripts/mock_upstream.py" \
        --port "${MOCK_PORT}" \
        --latency "${MOCK_LATENCY}" &
    MOCK_PID=$!
    PIDS_TO_KILL+=("${MOCK_PID}")
    sleep 1

    # Verify mock is up
    if ! kill -0 "${MOCK_PID}" 2>/dev/null; then
        echo "ERROR: Mock upstream failed to start."
        exit 1
    fi
    echo "  Mock PID: ${MOCK_PID}"
    echo ""
fi

# ---------------------------------------------------------------------------
# Wait for gate-server to be ready
# ---------------------------------------------------------------------------

echo "=== Checking gate-server availability ==="
MAX_WAIT=30
for i in $(seq 1 ${MAX_WAIT}); do
    if curl -sf "${API_URL}/health" >/dev/null 2>&1; then
        echo "  gate-server is ready."
        break
    fi
    if [[ "$i" -eq "${MAX_WAIT}" ]]; then
        echo "WARNING: gate-server not responding at ${API_URL}/health after ${MAX_WAIT}s."
        echo "  Proceeding anyway (k6 will report connection errors)."
    fi
    sleep 1
done
echo ""

# ---------------------------------------------------------------------------
# Run k6 load test
# ---------------------------------------------------------------------------

echo "=== Running k6 load test ==="
echo "  Output: ${RESULTS_DIR}/${TIMESTAMP}.json"
echo ""

k6 run \
    -e "API_URL=${API_URL}" \
    -e "API_KEY=${API_KEY}" \
    --out "json=${RESULTS_DIR}/${TIMESTAMP}.json" \
    "${ROOT_DIR}/bench/k6/chat_load.js"

K6_EXIT=$?

echo ""
echo "=== Load test complete (exit=${K6_EXIT}) ==="
echo "  Raw results:  ${RESULTS_DIR}/${TIMESTAMP}.json"
echo "  Summary:      ${RESULTS_DIR}/summary.json"

exit ${K6_EXIT}
