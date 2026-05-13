# Load Testing Infrastructure

Benchmark harness for verifying the **50k rpm** throughput target for kooix-gate.

## Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| [k6](https://grafana.com/docs/k6/) | ≥ 0.50 | `brew install k6` / `snap install k6` |
| Python 3 | ≥ 3.10 | System package or `pyenv` |
| Docker + Compose | ≥ 24 | [docs.docker.com](https://docs.docker.com/get-docker/) |
| Rust toolchain | ≥ 1.85 | For criterion micro-benchmarks |

## Quick Start (Local)

```bash
# 1. Start infra (Postgres + Redis)
docker compose -f docker-compose.dev.yml up -d

# 2. Run gate-server (in another terminal)
KOOIX_PROVIDER_BASE_URL=http://localhost:9999 cargo run --release --bin gate-server

# 3. Run the load test (starts mock upstream automatically)
./bench/scripts/run_bench.sh
```

## Docker Compose (All-in-One)

```bash
# Bring up entire stack + run k6
docker compose -f bench/docker-compose.bench.yml up --abort-on-container-exit k6

# Customize mock latency
MOCK_LATENCY=100 docker compose -f bench/docker-compose.bench.yml up --abort-on-container-exit k6

# Teardown
docker compose -f bench/docker-compose.bench.yml down -v
```

## Scripts

| File | Purpose |
|------|---------|
| `k6/chat_load.js` | Main k6 load test — ramp-up + constant 833 req/s |
| `k6/config.json` | Test parameters (reference, not loaded by k6) |
| `scripts/mock_upstream.py` | Mock OpenAI-compatible LLM server |
| `scripts/run_bench.sh` | Orchestrator — starts mock, waits for gate, runs k6 |
| `docker-compose.bench.yml` | Full containerized bench stack |
| `Dockerfile.mock` | Container image for mock upstream |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `API_URL` | `http://localhost:8080` | Gate server URL |
| `API_KEY` | `sk-kg-test-key` | API key for auth |
| `MOCK_PORT` | `9999` | Mock upstream port |
| `MOCK_LATENCY` | `50` | Mock response latency (ms) |
| `SKIP_MOCK` | _(empty)_ | Set to `1` to skip mock startup |

## Mock Upstream

The mock server (`mock_upstream.py`) provides:

- **Non-streaming**: instant JSON response with realistic structure
- **Streaming**: SSE chunks mimicking OpenAI's streaming format
- **Configurable latency**: `--latency <ms>` flag
- **Health check**: `GET /health`
- **Thread-per-request**: handles concurrent connections

```bash
# Standalone
python3 bench/scripts/mock_upstream.py --port 9999 --latency 100
```

## Test Scenarios

### Phase 1: Ramp-up (5 min)
Gradual increase from 10 to 1000 VUs, then cool-down. Identifies the
throughput curve and where latency starts degrading.

### Phase 2: Constant High Load (2 min)
Fixed arrival rate of **833 req/s** (≈ 50,000 rpm). Validates sustained
throughput at the target rate.

### Traffic Mix
- **40%** short messages (< 20 tokens)
- **35%** medium messages (~100 tokens)
- **20%** long messages (~500 tokens)
- **5%** code generation requests
- **20%** of all requests use streaming
- Model distribution: gpt-4o, gpt-4o-mini

## Targets

| Metric | Threshold |
|--------|-----------|
| Throughput | ≥ 50,000 requests/min |
| p50 latency | < 200ms |
| p95 latency | < 500ms |
| p99 latency | < 1,000ms |
| Error rate | < 1% |

## Interpreting Results

After a run, check:

1. **stdout** — k6 prints a summary table with pass/fail per threshold
2. **`bench/results/summary.json`** — machine-readable metrics snapshot
3. **`bench/results/<timestamp>.json`** — full k6 JSON output (every data point)

Key things to look for:

- `http_req_duration` p95/p99 — are they within thresholds?
- `errors` rate — any non-200 responses?
- `http_reqs` count/rate — did we sustain target throughput?
- `vus_max` — did k6 need to allocate more VUs than expected?

## Criterion Micro-Benchmarks

For CPU-bound routing logic (no I/O):

```bash
# Run all benchmarks
cargo bench --package gate-providers

# Run specific benchmark
cargo bench --package gate-providers -- routing

# Generate HTML report
# → target/criterion/report/index.html
```
