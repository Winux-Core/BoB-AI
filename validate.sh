#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

if [ ! -f .env ]; then
  echo "Missing .env file (needed for auth validation)."
  exit 1
fi

BOB_API_TOKEN="$(
  awk -F= '
    $0 ~ /^[[:space:]]*#/ { next }
    $1 == "BOB_API_TOKEN" {
      sub(/^[[:space:]]+/, "", $2)
      sub(/[[:space:]]+$/, "", $2)
      print substr($0, index($0, "=") + 1)
      exit
    }
  ' .env
)"

if [ -z "$BOB_API_TOKEN" ]; then
  echo "BOB_API_TOKEN is missing in .env."
  exit 1
fi
if ! [[ "$BOB_API_TOKEN" =~ ^[A-Za-z0-9._~-]{16,}$ ]]; then
  echo "BOB_API_TOKEN contains unsupported characters or is too short."
  echo "Use only [A-Za-z0-9._~-] and at least 16 chars."
  echo "Example: openssl rand -hex 32"
  exit 1
fi

echo "Waiting for API health endpoint..."
for _ in $(seq 1 45); do
  if curl -sf "http://127.0.0.1:8787/healthz" >/dev/null; then
    break
  fi
  sleep 2
done

if ! curl -sf "http://127.0.0.1:8787/healthz" >/dev/null; then
  echo "API is not healthy on http://127.0.0.1:8787/healthz"
  docker compose ps || true
  exit 1
fi

echo "Validating auth protection (expecting 401 without token)..."
unauth_code="$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:8787/config")"
if [ "$unauth_code" != "401" ]; then
  echo "Expected 401 from /config without token, got: $unauth_code"
  exit 1
fi

echo "Validating authenticated config access..."
auth_code="$(curl -s -o /dev/null -w "%{http_code}" -H "x-api-key: ${BOB_API_TOKEN}" "http://127.0.0.1:8787/config")"
if [ "$auth_code" != "200" ]; then
  echo "Expected 200 from /config with token, got: $auth_code"
  exit 1
fi

echo "Validation passed."
