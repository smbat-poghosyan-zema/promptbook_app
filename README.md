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

## Offline sandbox note

This scaffold includes local command shims for `tauri`, `vitest`, `eslint`, and `cargo` so verification can run in a no-network environment without Node/Rust package downloads. In a normal development machine, replace these shims with the official dependencies (`@tauri-apps/*`, Vite, React, Vitest, ESLint, Prettier, and Rust toolchain).
