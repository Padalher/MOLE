#!/usr/bin/env bash
# mole-core (file reading) must not depend on the user interface or on graphics libraries.
set -euo pipefail

# Crates in mole-core's dependency tree; "ERROR" if cargo tree fails.
deps=$(cargo tree -q -p mole-core -e normal,build,dev --prefix none --format '{p}' | awk '{print $1}' | sort -u || echo ERROR)
bad=$(echo "$deps" | grep -xE 'mole|x11rb.*|ERROR' || true)

if [ -n "$bad" ]; then
  echo "FAIL  mole-core depends on: $(echo $bad)" >&2
  exit 1
fi
echo "ok    mole-core has no UI or graphics dependencies"
