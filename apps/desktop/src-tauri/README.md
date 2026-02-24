# Promptbook Desktop App (Tauri)

A Linux-first desktop app for authoring, validating, and running Promptbook workflows.

## MVP Features

- Create and edit Promptbook files.
- Run Promptbook steps locally and view results.
- Validate Promptbook format/schema before execution.
- Basic run history with logs per step.

## Required Toolchain

- Node.js `20+`
- `pnpm`
- Rust stable toolchain (`rustup`, `cargo`)
- Tauri Linux prerequisites (WebKitGTK, GTK3, build tools)

Ubuntu/Debian example:

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf build-essential curl wget file pkg-config
```

## Run, Test, Build

From the desktop app workspace (or monorepo root where scripts are exposed):

```bash
pnpm install
pnpm tauri dev
pnpm test
pnpm tauri build
```

If scripts are not wired yet, use this as the target command set for upcoming setup.
