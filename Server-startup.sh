#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

if [ ! -f .env ]; then
  echo "Missing .env file."
  echo "Create it from .env.example and set required values."
  exit 1
fi

if command -v systemctl >/dev/null 2>&1; then
  if ! systemctl --user is-active podman.socket >/dev/null 2>&1; then
    echo "Starting podman socket..."
    systemctl --user start podman.socket
  fi
fi

echo "Starting BoB services..."
docker compose up --build -d

echo "Running startup validation..."
"$ROOT_DIR/validate.sh"

echo ""
echo "BoB API ready at http://127.0.0.1:8787"
echo "Stop with: ./Server-shutdown.sh"
