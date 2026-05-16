# Codex Session Manager Frontend

React + Vite frontend for the Codex Session Manager. The same UI runs in two modes:

- Browser mode: `npm run dev`, using the Rust HTTP backend through `/api`.
- Desktop mode: `npm run tauri:dev`, using Tauri commands from `src-tauri`.

## Run

```bash
npm install
npm run dev
```

For the desktop MVP:

```bash
npm run tauri:dev
```

Build the desktop app bundle:

```bash
npm run tauri:build
```

`tauri:build` emits the app bundle only. Run `npm run tauri:bundle` when you need platform installer artifacts such as DMG.

The activity summary feature calls the local `codex` CLI in non-interactive mode. Set `CSM_CODEX_BIN` before launching the backend or desktop app if `codex` is not discoverable from `PATH`.

## Verify

```bash
npm run lint
npm run build

cd src-tauri
cargo fmt --check
cargo check

cd ..
npm run tauri:build
```
