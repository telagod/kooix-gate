#!/usr/bin/env bash
set -euo pipefail

# Build the v1 WASM transform plugin example.
# Requires: rustup target wasm32-unknown-unknown, wasm-tools

cd "$(dirname "$0")"

echo "Building core module..."
cargo build --target wasm32-unknown-unknown --release

CORE=target/wasm32-unknown-unknown/release/wasm_transform_v1_example.wasm
OUT=plugin.component.wasm

echo "Componentizing..."
wasm-tools component new "$CORE" -o "$OUT"

echo "Done: $OUT ($(du -h "$OUT" | cut -f1))"
wasm-tools component wit "$OUT"
