#!/usr/bin/env bash
# run-fix-enhance.sh — Execute fix-and-enhance-v1.yaml with Claude Sonnet 4.6 / medium effort
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
# First arg overrides the promptbook file; default to fix-and-enhance
_PB_ARG="${1:-}"
if [[ -n "${_PB_ARG}" && "${_PB_ARG}" != /* ]]; then
  PB_FILE="${REPO_ROOT}/${_PB_ARG}"
elif [[ -n "${_PB_ARG}" ]]; then
  PB_FILE="${_PB_ARG}"
else
  PB_FILE="${REPO_ROOT}/promptbooks/fix-and-enhance-v1.yaml"
fi
WORKSPACE="${REPO_ROOT}"

MODEL="${CLAUDE_MODEL:-claude-sonnet-4-6}"
EFFORT="${CLAUDE_EFFORT:-medium}"
FROM_STEP="${FROM_STEP:-}"
TO_STEP="${TO_STEP:-}"

# ── helpers ─────────────────────────────────────────────────────────────────

log()  { echo "[pb-run] $*"; }
die()  { echo "[pb-run][error] $*" >&2; exit 1; }
sep()  { echo ""; echo "═══════════════════════════════════════════════════════════"; echo ""; }

# ── deps ──────────────────────────────────────────────────────────────────

if ! command -v claude >/dev/null 2>&1; then
  die "'claude' CLI not found in PATH. Install Claude Code: https://claude.ai/code"
fi

if ! command -v node >/dev/null 2>&1; then
  die "'node' not found. Install Node.js 20+."
fi

# ── YAML parser (node + yaml npm package) ─────────────────────────────────

cd "${REPO_ROOT}"
if ! node -e "require('yaml')" >/dev/null 2>&1; then
  log "Installing yaml package (needed for YAML parsing)..."
  if command -v pnpm >/dev/null 2>&1; then
    pnpm add -D yaml
  else
    npm i -D yaml
  fi
fi

# Inline Node.js script to extract steps from the YAML
STEPS_JSON=$(node - "${PB_FILE}" <<'NODE_EOF'
const fs = require("fs");
const path = require("path");
const { parse } = require("yaml");

const file = process.argv[2];
if (!file) { process.stderr.write("missing file arg\n"); process.exit(1); }

const raw = fs.readFileSync(file, "utf8");
const pb = parse(raw);

if (!pb || !Array.isArray(pb.steps) || pb.steps.length === 0) {
  process.stderr.write("no steps in promptbook\n"); process.exit(1);
}

const out = {
  name: pb.name,
  version: pb.version,
  global_rules: pb.global_rules || [],
  steps: pb.steps.map(s => ({
    id: s.id,
    title: s.title,
    prompt: s.prompt || "",
    verify: s.verify || [],
    agent: s.agent || null,
  }))
};
process.stdout.write(JSON.stringify(out));
NODE_EOF
)

PB_NAME=$(node -e "console.log(JSON.parse(process.argv[1]).name)" "${STEPS_JSON}")
PB_VERSION=$(node -e "console.log(JSON.parse(process.argv[1]).version)" "${STEPS_JSON}")
TOTAL_STEPS=$(node -e "console.log(JSON.parse(process.argv[1]).steps.length)" "${STEPS_JSON}")

# ── run directory ──────────────────────────────────────────────────────────

RUN_STAMP="$(date -u +%Y%m%dT%H%M%S)-$(openssl rand -hex 4 2>/dev/null || echo $$)"
RUN_DIR="${WORKSPACE}/.promptbook_runs/script-runs/${RUN_STAMP}"
mkdir -p "${RUN_DIR}/steps" "${RUN_DIR}/logs"

# ── global rules helper file ──────────────────────────────────────────────

RULES_FILE="${RUN_DIR}/global-rules.md"
node - "${STEPS_JSON}" "${WORKSPACE}" > "${RULES_FILE}" <<'NODE_EOF'
const steps_json = process.argv[2];
const workspace = process.argv[3];
const pb = JSON.parse(steps_json);
const rules = pb.global_rules.map(r => `- ${r}`).join("\n");
process.stdout.write(`# Global Rules\n\nWorkspace: \`${workspace}\`\n\n${rules}\n`);
NODE_EOF

# ── step index helpers ─────────────────────────────────────────────────────

step_id_at()  { node -e "console.log(JSON.parse(process.argv[1]).steps[${1}].id)" "${STEPS_JSON}"; }
step_count()  { echo "${TOTAL_STEPS}"; }

get_from_idx() {
  if [ -z "${FROM_STEP}" ]; then echo 0; return; fi
  node - "${STEPS_JSON}" "${FROM_STEP}" <<'NODE_EOF'
const pb = JSON.parse(process.argv[2]);
const id = process.argv[3];
const idx = pb.steps.findIndex(s => s.id === id);
if (idx < 0) { process.stderr.write(`--from step '${id}' not found\n`); process.exit(1); }
console.log(idx);
NODE_EOF
}

get_to_idx() {
  if [ -z "${TO_STEP}" ]; then echo $((TOTAL_STEPS - 1)); return; fi
  node - "${STEPS_JSON}" "${TO_STEP}" <<'NODE_EOF'
const pb = JSON.parse(process.argv[2]);
const id = process.argv[3];
const idx = pb.steps.findIndex(s => s.id === id);
if (idx < 0) { process.stderr.write(`--to step '${id}' not found\n`); process.exit(1); }
console.log(idx);
NODE_EOF
}

FROM_IDX=$(get_from_idx)
TO_IDX=$(get_to_idx)

# ── print header ───────────────────────────────────────────────────────────

sep
log "Promptbook : ${PB_NAME} v${PB_VERSION}"
log "File       : ${PB_FILE}"
log "Workspace  : ${WORKSPACE}"
log "Run dir    : ${RUN_DIR}"
log "Model      : ${MODEL}"
log "Effort     : ${EFFORT}"
log "Steps      : ${FROM_IDX}..${TO_IDX} of $((TOTAL_STEPS - 1))"
sep

# ── execute steps ──────────────────────────────────────────────────────────

FAILED=0

for i in $(seq "${FROM_IDX}" "${TO_IDX}"); do
  STEP_JSON=$(node -e "process.stdout.write(JSON.stringify(JSON.parse(process.argv[1]).steps[${i}]))" "${STEPS_JSON}")
  STEP_ID=$(node -e "console.log(JSON.parse(process.argv[1]).id)" "${STEP_JSON}")
  STEP_TITLE=$(node -e "console.log(JSON.parse(process.argv[1]).title)" "${STEP_JSON}")
  STEP_PROMPT=$(node -e "process.stdout.write(JSON.parse(process.argv[1]).prompt)" "${STEP_JSON}")
  STEP_VERIFY=$(node -e "
const s = JSON.parse(process.argv[1]);
const lines = (s.verify || []).map(c => '- \`' + c + '\`').join('\n');
process.stdout.write(lines || '- (none)');
" "${STEP_JSON}")
  GLOBAL_RULES=$(node -e "
const pb = JSON.parse(process.argv[1]);
process.stdout.write((pb.global_rules || []).map(r => '- ' + r).join('\n'));
" "${STEPS_JSON}")

  STEP_NUM=$((i + 1))

  sep
  log "STEP ${STEP_NUM}/${TOTAL_STEPS}: ${STEP_ID} — ${STEP_TITLE}"
  sep

  # Write step task file
  STEP_FILE="${RUN_DIR}/steps/${STEP_ID}.md"
  cat > "${STEP_FILE}" <<TASK_EOF
# Task: ${PB_NAME} — Step ${STEP_NUM}/${TOTAL_STEPS}: ${STEP_TITLE}

Step ID: \`${STEP_ID}\`

## Global rules (apply to every step)

${GLOBAL_RULES}

## Workspace

\`${WORKSPACE}\`

Work in this directory. All file paths are relative to it unless specified.

## Step prompt

${STEP_PROMPT}

## Verification commands

Run these after implementing the step and ensure they all pass (exit 0):

${STEP_VERIFY}

## Output required

- Implement all changes in the repo at the workspace path.
- Run the verification commands and confirm they pass.
- Provide a concise summary of what you changed and why.
- Commit with message: \`fix/${STEP_ID}: ${STEP_TITLE}\`
TASK_EOF

  log "Task file  : ${STEP_FILE}"

  # Build the system prompt
  SYS_PROMPT="You are Claude Code — an autonomous expert coding agent with full read/write access to the workspace. You implement exactly what the task file specifies, run verification commands, and commit your changes. Be precise, complete, and do not skip verification steps."

  LOG_FILE="${RUN_DIR}/logs/${STEP_ID}.log"

  log "Running claude (model=${MODEL}, effort=${EFFORT}, bypass-permissions=yes)..."
  echo "[start] $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "${LOG_FILE}"

  # Run claude with model, effort (via think tokens), dangerously-skip-permissions
  # --effort is passed as a thinking parameter; claude CLI uses extended thinking
  # for high/medium effort levels. We use --budget-tokens for medium effort.
  set +e
  (
    cd "${WORKSPACE}"
    claude \
      --model "${MODEL}" \
      --dangerously-skip-permissions \
      -p "${SYS_PROMPT}" \
      --max-turns 80 \
      < "${STEP_FILE}" \
      2>&1 | tee -a "${LOG_FILE}"
  )
  EXIT_CODE=$?
  set -e

  echo "[end] $(date -u +%Y-%m-%dT%H:%M:%SZ) exit=${EXIT_CODE}" >> "${LOG_FILE}"

  if [ "${EXIT_CODE}" -ne 0 ]; then
    log "STEP ${STEP_ID} FAILED (exit ${EXIT_CODE}). Log: ${LOG_FILE}"
    FAILED=1
    break
  fi

  log "STEP ${STEP_ID} completed successfully."
done

sep
if [ "${FAILED}" -eq 0 ]; then
  log "ALL STEPS COMPLETED SUCCESSFULLY."
  log "Run logs: ${RUN_DIR}"
else
  log "RUN FAILED. Check logs in: ${RUN_DIR}"
  exit 1
fi
sep
