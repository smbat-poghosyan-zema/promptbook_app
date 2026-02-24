# MVP Architecture Overview

## Scope

The MVP delivers a local-first desktop application for creating and executing Promptbook workflows.

## High-Level Components

- **UI (Frontend)**: Tauri webview app (TypeScript/React) for editing Promptbook files, launching runs, and displaying logs/status.
- **Core Domain**: Shared Promptbook model, validation rules, and run state management.
- **Runner Orchestrator (Tauri/Rust)**: Receives run requests from UI, executes steps safely, streams logs/events back to UI.
- **Filesystem Adapter**: Reads/writes Promptbook YAML files and run artifacts on local disk.

## Data Flow

1. User opens/creates Promptbook in UI.
2. UI validates structure and sends normalized run request to Tauri backend.
3. Rust orchestrator executes steps sequentially (or configured strategy), captures output, status, and timing.
4. Events are emitted to UI for live progress updates.
5. Final run report is persisted locally and rendered in run history.

## Boundaries

- UI handles interaction and rendering only.
- Rust backend handles process execution, OS integration, and safety constraints.
- Shared schema/contracts prevent drift between UI and backend.

## Non-Goals (MVP)

- Cloud synchronization
- Multi-user collaboration
- Remote execution agents

## Quality Baseline

- Deterministic run status per step (`pending`, `running`, `passed`, `failed`, `skipped`).
- Input validation before execution.
- Structured logging for troubleshooting.
