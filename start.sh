#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Ensure required env file exists for secure defaults
if [ ! -f .env ]; then
  echo "Missing .env file."
  echo "Create one from .env.example and set BOB_API_TOKEN + BOB_API_CORS_ORIGIN."
  exit 1
fi

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
    echo "✓ Ollama and Postgres are internal-only (not host-exposed)"
    echo ""
    echo "Stop with: docker compose down"
    exit 0
  fi
  sleep 2
done

kill $LOG_PID 2>/dev/null || true
echo "API did not become healthy in 60s. Check: docker compose logs"
exit 1
