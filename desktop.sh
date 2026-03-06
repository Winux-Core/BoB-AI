#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

# Install root deps (tauri cli)
if [ ! -d node_modules ]; then
  echo "Installing root dependencies..."
  npm install
fi

# Install web frontend deps
if [ ! -d web/node_modules ]; then
  echo "Installing web dependencies..."
  npm --prefix web install
fi

# Check if API is reachable
if curl -sf http://127.0.0.1:8787/healthz &>/dev/null; then
  echo "✓ BoB API detected at http://127.0.0.1:8787"
else
  echo "⚠ BoB API not running. Start it first with ./Server-startup.sh"
  echo "  Launching desktop anyway (configure remote URL in Settings)..."
fi

echo "Starting BoB desktop app..."
npx tauri dev --config desktop/tauri.conf.json
