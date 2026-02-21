# promptbook-runner-linux-desktop

Desktop app for running local Promptbook workflows on Linux using a Tauri shell (Rust backend + web frontend).

## Workspace layout

- `apps/desktop`: Tauri + React + TypeScript desktop UI shell
- `packages/shared`: Shared Promptbook and IPC schemas/types (`zod`)

## Toolchain requirements

- Node.js 20+
- pnpm 9+
- Rust stable toolchain (`rustup`, `cargo`)
- Tauri prerequisites for Linux:
  - `webkit2gtk`
  - `gtk3`
  - `librsvg2`
  - https://tauri.app/start/prerequisites/

## Commands

```bash
pnpm install
pnpm dev
pnpm test
pnpm lint
pnpm build
```

## How to Configure Agents

The Rust backend defines an adapter contract for agent CLIs:
- `codex`: runs `codex exec --full-auto --sandbox workspace-write "<instructions>"`
- `claude`: runs `bash -lc 'cat <step-file> | claude -p "<system-prompt>"'`
- `copilot`: runs `copilot -p "<prompt>"` with safe defaults (no allow-all-tools unless explicitly enabled)
- `dry-run`: test adapter that emits final output on stdout and progress on stderr

Each adapter returns a `CommandSpec` with:
- `program`
- `args`
- `cwd` (workspace directory)

Tool approval and execution behavior differ per agent CLI. Keep adapter-specific safety defaults in place and only broaden permissions intentionally.

## Offline sandbox note

This scaffold includes local command shims for `tauri`, `vitest`, `eslint`, and `cargo` so verification can run in a no-network environment without Node/Rust package downloads. In a normal development machine, replace these shims with the official dependencies (`@tauri-apps/*`, Vite, React, Vitest, ESLint, Prettier, and Rust toolchain).
