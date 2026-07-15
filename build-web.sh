#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

mkdir -p public
cargo build --release --target wasm32-unknown-unknown --lib
cp target/wasm32-unknown-unknown/release/loopit.wasm public/loopit.wasm

echo "Built public/loopit.wasm"
