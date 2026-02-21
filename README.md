# promptbook-runner-linux-desktop

Desktop app for running local Promptbook workflows on Linux using a Tauri shell (Rust backend + web frontend).

## Workspace layout

- `apps/desktop`: Tauri + React + TypeScript desktop UI shell
- `packages/shared`: Shared Promptbook and IPC schemas/types (`zod`)

## Install prerequisites

1. Install toolchains:
   - Node.js 20+
   - pnpm 9+
   - Rust stable (`rustup`, `cargo`)
2. Install Linux system packages required by Tauri:
   - Follow the distro-specific instructions at https://tauri.app/start/prerequisites/
3. Install workspace dependencies:

```bash
pnpm install
```

## Run dev

```bash
pnpm dev
```

## Run tests

```bash
pnpm test
pnpm lint
```

## Build release

```bash
pnpm build
```

On a full local setup, Tauri release artifacts are produced under `apps/desktop/src-tauri/target/release/bundle/`.

## Agent setup notes

Agent adapter logic lives in `apps/desktop/src-tauri/src/agent_adapter.rs`. Supported adapters are:
- `codex`
- `claude`
- `copilot`
- `dry-run`

Each adapter returns a `CommandSpec` with:
- `program`
- `args`
- `cwd`

Use per-step or default `agent` values from promptbook YAML (`promptbook/v1`) to choose adapters at runtime.

## Security guidance

- Keep least-privilege defaults in adapter commands.
- Keep `copilot` in safe mode unless explicit expansion is required.
- Prefer `workspace-write` sandboxes for agent execution.
- Review step prompts and verification commands before running untrusted promptbooks.
- Do not broaden filesystem/network permissions without a concrete need and audit trail.

## Offline sandbox note

This scaffold includes local command shims for `tauri`, `vitest`, `eslint`, and `cargo` so verification can run in a no-network environment without package downloads. On a normal development machine, use the official dependencies (`@tauri-apps/*`, Vite, React, Vitest, ESLint, Prettier, and Rust toolchain).
