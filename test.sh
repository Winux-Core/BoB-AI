#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

cleanup() {
  "$ROOT_DIR/Server-shutdown.sh" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "Stopping any existing stack..."
"$ROOT_DIR/Server-shutdown.sh" || true

echo "Removing all target directories..."
find "$ROOT_DIR" -type d -name target -prune -exec rm -rf {} +

echo "Running full build pipeline..."
"$ROOT_DIR/build.sh"

echo "Running Rust tests..."
cargo test --workspace --all-targets

echo "Running CLI smoke check..."
cargo run -p bob-cli -- --help >/dev/null

echo "Starting server stack..."
"$ROOT_DIR/Server-startup.sh"

echo "Running runtime validation..."
"$ROOT_DIR/validate.sh"

echo "All tests/build/startup validations passed."
