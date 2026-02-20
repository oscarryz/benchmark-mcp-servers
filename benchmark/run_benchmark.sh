#!/usr/bin/env bash
set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RESULTS_DIR="$SCRIPT_DIR/results/$TIMESTAMP"

# Servers to benchmark (name:container:port)
declare -A SERVERS=(
    [python]="mcp-python-server:8082"
    [go]="mcp-go-server:8081"
    [nodejs]="mcp-nodejs-server:8083"
    [java]="mcp-java-server:8080"
    [rust]="mcp-rust-server:8084"
)
ALL_SERVICES="python-server go-server nodejs-server java-server rust-server"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}   $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERR]${NC}  $*"; }

# ─── Functions ────────────────────────────────────────────────────────

wait_for_health() {
    local port=$1
    local name=$2
    local max_wait=60
    local elapsed=0

    info "Waiting for $name to be ready (port $port)..."
    while true; do
        # Try /health (Go, Node.js, Rust)
        if curl -sf -m 2 "http://localhost:$port/health" > /dev/null 2>&1; then
            break
        fi
        # Try /actuator/health (Java)
        if curl -sf -m 2 "http://localhost:$port/actuator/health" > /dev/null 2>&1; then
            break
        fi
        # Try MCP endpoint (Python — no health endpoint, but MCP responds)
        if curl -sf -m 2 -X POST "http://localhost:$port/mcp" \
            -H "Content-Type: application/json" \
            -H "Accept: application/json, text/event-stream" \
            -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"health","version":"1.0"}}}' \
            > /dev/null 2>&1; then
            break
        fi
        sleep 1
        elapsed=$((elapsed + 1))
        if [ $elapsed -ge $max_wait ]; then
            error "$name failed to start after ${max_wait}s"
            return 1
        fi
    done
    ok "$name is ready (${elapsed}s)"
}

warmup() {
    local url=$1
    local name=$2
    info "Warming up $name (10 requests)..."

    for i in $(seq 1 10); do
        curl -sf -X POST "$url" \
            -H "Content-Type: application/json" \
            -H "Accept: application/json, text/event-stream" \
            -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"warmup","version":"1.0"}}}' \
            > /dev/null 2>&1 || true
    done
    ok "Warmup complete"
}

stop_all_servers() {
    info "Stopping all MCP server containers..."
    cd "$PROJECT_DIR"
    docker compose stop $ALL_SERVICES 2>/dev/null || true
    sleep 2
    ok "All servers stopped"
}

start_server() {
    local service=$1
    info "Starting $service..."
    cd "$PROJECT_DIR"
    docker compose up -d "$service" 2>/dev/null
}

benchmark_server() {
    local name=$1
    local container_port=${SERVERS[$name]}
    local container="${container_port%%:*}"
    local port="${container_port##*:}"
    local service="${name}-server"
    local url="http://localhost:$port/mcp"
    local server_results="$RESULTS_DIR/$name"

    mkdir -p "$server_results"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  BENCHMARKING: ${name^^}"
    echo "  Container: $container | Port: $port | URL: $url"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # 1. Stop all servers, start only the target
    stop_all_servers
    start_server "$service"

    # 2. Wait for health
    if ! wait_for_health "$port" "$name"; then
        error "Skipping $name — failed to start"
        return 1
    fi

    # 3. Warmup
    warmup "$url" "$name"

    # 4. Start stats collector in background
    info "Starting Docker stats collector..."
    python3 "$SCRIPT_DIR/collect_stats.py" "$container" "$server_results/stats.json" 1.0 &
    local stats_pid=$!
    sleep 1

    # 5. Run k6 benchmark
    info "Running k6 benchmark (50 VUs, 5m)..."
    k6 run \
        --env SERVER_URL="$url" \
        --env SERVER_NAME="$name" \
        --env OUTPUT_PATH="$server_results/k6.json" \
        "$SCRIPT_DIR/benchmark.js" \
        2>&1 | tee "$server_results/k6_console.log"

    # 6. Stop stats collector
    info "Stopping stats collector..."
    kill "$stats_pid" 2>/dev/null || true
    wait "$stats_pid" 2>/dev/null || true

    ok "Benchmark complete for ${name^^}"
}

# ─── Main ─────────────────────────────────────────────────────────────

main() {
    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║           MCP SERVERS BENCHMARK SUITE                      ║"
    echo "╠══════════════════════════════════════════════════════════════╣"
    echo "║  VUs: 10 | Duration: 5m | CPU: 1 core | RAM: 1GB          ║"
    echo "║  Servers: python, go, nodejs, java                         ║"
    echo "║  Results: $RESULTS_DIR"
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo ""

    mkdir -p "$RESULTS_DIR"

    # Ensure mock-api is running
    info "Ensuring mock-api is running..."
    cd "$PROJECT_DIR"
    docker compose up -d mock-api 2>/dev/null
    ok "mock-api is up"

    # Benchmark each server
    for name in python go nodejs java rust; do
        benchmark_server "$name" || warn "Failed to benchmark $name, continuing..."
    done

    # Stop all servers
    stop_all_servers

    # Consolidate results
    echo ""
    info "Consolidating results..."
    python3 "$SCRIPT_DIR/consolidate.py" "$RESULTS_DIR"

    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║  BENCHMARK COMPLETE                                        ║"
    echo "╠══════════════════════════════════════════════════════════════╣"
    echo "║  Results: $RESULTS_DIR"
    echo "║  Summary: $RESULTS_DIR/summary.json"
    echo "╚══════════════════════════════════════════════════════════════╝"
}

main "$@"
