#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

DOWN_ARGS=(down --remove-orphans)
if [ "${1:-}" = "--volumes" ]; then
  DOWN_ARGS+=(--volumes)
fi

echo "Stopping BoB services..."
docker compose "${DOWN_ARGS[@]}"
