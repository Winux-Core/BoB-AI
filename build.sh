#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

echo "Installing/updating JS dependencies..."
npm install
npm --prefix web install

echo "Building web UI..."
npm run --prefix web build

echo "Building Rust workspace..."
cargo build --workspace

echo "Building container image..."
docker compose build bob-api

echo "Build complete."
