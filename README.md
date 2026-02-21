# promptbook-runner-linux-desktop

Desktop app for running local Promptbook workflows on Linux using a Tauri shell (Rust backend + web frontend).

## MVP features

- Open and run Promptbook files from the local filesystem.
- Show step-by-step run status and logs.
- Store run artifacts in a local workspace folder.
- Support YAML v1 promptbook format with deterministic step execution.

## Toolchain requirements

- Node.js 20+
- pnpm 9+
- Rust stable toolchain (`rustup`, `cargo`)
- Tauri prerequisites for Linux:
  - `webkit2gtk`
  - `gtk3`
  - `librsvg2`
  - See: https://tauri.app/start/prerequisites/

## Commands

The scripts below are the intended MVP workflow; concrete implementations are added in later steps.

```bash
pnpm install
pnpm run dev
pnpm run test
pnpm run build
```
