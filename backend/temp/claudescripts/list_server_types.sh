#!/usr/bin/env bash
# List Hetzner Cloud server types (CPX series) with specs
# Requires HETZNER_API_TOKEN in .env or environment
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Load token from .env
if [[ -f "$PROJECT_ROOT/.env" ]]; then
    HETZNER_API_TOKEN=$(grep '^HETZNER_API_TOKEN=' "$PROJECT_ROOT/.env" | cut -d'=' -f2-)
fi

if [[ -z "${HETZNER_API_TOKEN:-}" ]]; then
    echo "ERROR: HETZNER_API_TOKEN not set" >&2
    exit 1
fi

echo "=== CPX Server Types ==="
curl -s -H "Authorization: Bearer $HETZNER_API_TOKEN" \
    "https://api.hetzner.cloud/v1/server_types?per_page=50" \
    | jq -r '.server_types[] | select(.name | startswith("cpx")) | "\(.name)\t\(.cores) vCPU\t\(.memory)GB RAM\t\(.disk)GB disk\t\(.description)"'

echo ""
echo "=== Locations ==="
curl -s -H "Authorization: Bearer $HETZNER_API_TOKEN" \
    "https://api.hetzner.cloud/v1/locations" \
    | jq -r '.locations[] | "\(.name)\t\(.city)\t\(.network_zone)"'

echo ""
echo "=== Available Images (debian) ==="
curl -s -H "Authorization: Bearer $HETZNER_API_TOKEN" \
    "https://api.hetzner.cloud/v1/images?type=system&status=available&per_page=50" \
    | jq -r '.images[] | select(.name | startswith("debian")) | "\(.name)\t\(.description)\t\(.os_flavor)"'
