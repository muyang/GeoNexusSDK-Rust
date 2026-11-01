#!/usr/bin/env bash
# Refresh the vendored GeoCard schema from the upstream GeoNexus SDK repository.
#
# The Rust SDK is published standalone, but must validate against the *same*
# schema as the Python and TypeScript SDKs. The schema is embedded at compile
# time via include_str!, and a test asserts its type enum matches GEOCARD_TYPES.
#
# Upstream source: muyang/GeoNexus-SDK
# Last synced from commit: 21c6af7b2f8963c9ba7297d1dea0a8716fe47665
#
# Usage: ./scripts/sync-schema.sh /path/to/GeoNexus-SDK
set -euo pipefail
UPSTREAM="${1:?usage: sync-schema.sh /path/to/GeoNexus-SDK}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cp "$UPSTREAM/schemas/geocard.schema.json" "$ROOT/schemas/geocard.schema.json"
echo "synced from $UPSTREAM ($(git -C "$UPSTREAM" rev-parse --short HEAD 2>/dev/null || echo unknown))"
