# MVP Architecture Overview

## Goal

Build a Linux desktop Promptbook runner with a small, testable architecture and clear boundaries between UI, orchestration, and execution.

## High-level components

1. Tauri host (Rust)
- Owns application lifecycle, desktop windows, and secure OS access.
- Exposes a minimal command API to the frontend for filesystem and process-safe operations.

2. Frontend app (TypeScript)
- Presents promptbook selection, execution controls, run timeline, and logs.
- Sends run requests to the runner service and subscribes to status updates.

3. Runner service (TypeScript, shared app layer)
- Validates promptbook schema (YAML v1).
- Builds an execution plan from promptbook steps.
- Executes steps sequentially with deterministic state passing.
- Emits structured events (`run_started`, `step_started`, `step_finished`, `run_finished`, `run_failed`).

4. Persistence layer
- Stores run metadata and step outputs under a local workspace directory.
- Keeps implementation file-based in MVP for low complexity and easy debugging.

## Runtime flow

1. User selects a promptbook file.
2. Frontend requests validation + plan creation.
3. Runner executes steps in order and emits progress events.
4. Frontend renders status/logs in near-real-time.
5. Final artifacts are persisted and surfaced in the UI.

## Data model (MVP)

- `Promptbook`: version, metadata, ordered list of steps.
- `Run`: run id, timestamps, source promptbook path/version, status.
- `StepResult`: step id, status, input summary, output payload, timing, error info.

## Boundaries and safety

- Frontend never performs direct privileged filesystem operations; it calls Tauri commands.
- Runner logic remains framework-agnostic to enable direct unit tests outside the UI.
- Inputs are validated before execution; invalid promptbooks fail fast with actionable errors.

## Testing strategy (incremental)

- Unit tests for promptbook parsing/validation and step orchestration.
- Integration tests for end-to-end run flow with fixture promptbooks.
- UI smoke tests later for core interactions (load, run, inspect logs).
