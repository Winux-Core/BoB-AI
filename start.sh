#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Ensure podman socket is running
if ! systemctl --user is-active podman.socket &>/dev/null; then
  echo "Starting podman socket..."
  systemctl --user start podman.socket
fi

echo "Building and starting BoB stack..."
docker compose up --build -d

echo ""
echo "Waiting for services..."
docker compose logs -f bob-api-1 2>/dev/null &
LOG_PID=$!

# Wait for API to respond
for i in $(seq 1 30); do
  if curl -sf http://127.0.0.1:8787/healthz &>/dev/null; then
    kill $LOG_PID 2>/dev/null || true
    echo ""
    echo "✓ BoB API ready at http://127.0.0.1:8787"
    echo "✓ Ollama at http://127.0.0.1:11434"
    echo "✓ Postgres at localhost:5432"
    echo ""
    echo "Stop with: docker compose down"
    exit 0
  fi
  sleep 2
done

kill $LOG_PID 2>/dev/null || true
echo "API did not become healthy in 60s. Check: docker compose logs"
exit 1
