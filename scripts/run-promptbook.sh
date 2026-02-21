#!/usr/bin/env bash
set -euo pipefail

PB_FILE="${1:-promptbooks/build-promptbook-runner.v1.yaml}"
AGENT="${2:-codex}"
WORKSPACE="${3:-.}"

# Dependency used by pb-run.mjs for YAML parsing
if ! node -e "require('yaml')" >/dev/null 2>&1; then
  echo "[info] installing dev dependency: yaml"
  if command -v pnpm >/dev/null 2>&1; then
    pnpm add -D yaml
  else
    npm i -D yaml
  fi
fi

node scripts/pb-run.mjs --file "$PB_FILE" --agent "$AGENT" --workspace "$WORKSPACE"
