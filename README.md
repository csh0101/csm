# Codex Session Manager

Local workspace for the Codex Session Manager frontend and Rust backend.

## Project Layout

- `backend/` - Rust API service for local session scanning, metadata persistence, archive-delete, and restore.
- `frontend/` - React + Vite single-page app plus Tauri desktop shell.
- `frontend/src-tauri/` - Tauri v2 desktop entrypoint using Rust commands.
- `docs/codex-session-manager-prd.md` - Product requirements used to derive the backend API.

## Run Locally

Start the Rust backend:

```bash
cd backend
cargo run
```

The backend listens on `http://127.0.0.1:4000` by default. Useful environment variables:

- `CSM_BIND_ADDR` - host and port, default `127.0.0.1:4000`.
- `CSM_DATA_DIR` - metadata directory, default `~/.codex-session-manager`.
- `CSM_ARCHIVE_DIR` - archive-copy directory, default `$CSM_DATA_DIR/archive`.
- `CSM_MAX_PREVIEW_BYTES` - max text bytes loaded per scanned file, default `524288`.
- `CSM_STALE_AFTER_DAYS` - initial default for days since last modification before a session is considered stale, default `15`. The UI can update this value online and persists it in metadata.
- `CSM_CODEX_BIN` - optional path to the `codex` CLI used by AI activity summaries. If unset, the app searches `PATH`, common local install paths, and NVM node versions.
- `CSM_PEER_TOKEN` - required token for LAN peer read APIs. Peers must send it as `x-csm-peer-token` or `Authorization: Bearer`.
- `CSM_LAN_DISCOVERY` - controls mDNS LAN peer announcements and discovery, default `true`. Set to `false` to disable broadcasting.
- `CSM_PEER_DISPLAY_NAME` - optional display name advertised in LAN peer presence.

Start the frontend:

```bash
cd frontend
npm run dev
```

The Vite dev server proxies `/api` to `http://127.0.0.1:4000`. To call a different backend directly from the browser, set `VITE_API_BASE_URL` in the frontend environment.

Start the Tauri desktop app:

```bash
cd frontend
npm run tauri:dev
```

The desktop app reuses the React/Vite frontend. Session management calls Rust through Tauri commands; collaboration uses a local HTTP API started by the desktop shell so `/api/collaboration` and `/peer/*` behave the same as the standalone backend. By default, desktop metadata is stored in the platform app data directory, and the desktop shell reserves an available loopback port for its local API; set `CSM_DATA_DIR` to force the same metadata location as the standalone backend, or `CSM_BIND_ADDR` to force a specific collaboration API address.

For LAN collaboration, set `CSM_PEER_TOKEN` before launching so the collaboration tab can show the token others must use. mDNS discovery broadcasts are enabled by default; set `CSM_LAN_DISCOVERY=false` only when you want to turn them off. If other devices need to connect, set `CSM_BIND_ADDR` to a LAN-reachable bind address. Use a concrete LAN IP such as `192.168.1.12:4000` when you want the tab to show a directly shareable URL; `0.0.0.0:4000` listens on all interfaces, but peers must replace `0.0.0.0` with this machine's LAN IP.

Build the desktop app bundle:

```bash
cd frontend
npm run tauri:build
```

On macOS this produces a `.app` bundle. Use `npm run tauri:bundle` when you need the platform installer targets such as DMG; that path can require a full interactive macOS packaging environment.

## API Surface

- `GET /api/health`
- `GET /api/sessions`
- `POST /api/sessions/scan` with `{ "path": "/path/to/sessions" }`
- `PATCH /api/settings` with `{ "staleAfterDays": 15 }`
- `PATCH /api/sessions/{id}/labels` with `{ "labels": ["Backend"] }`
- `PATCH /api/sessions/{id}/notes` with `{ "notes": "..." }`
- `POST /api/summaries/activity` with `{ "days": 7, "language": "zh" }`
- `GET /api/collaboration`
- `PATCH /api/collaboration/share-policies/{projectId}`
- `POST /api/collaboration/peers/pair`
- `POST /api/collaboration/subscriptions`
- `POST /api/collaboration/summaries/baseline`
- `POST /api/collaboration/summaries/incremental`
- `POST /api/sessions/{id}/archive-delete`
- `POST /api/sessions/{id}/restore`

Peer read APIs are mounted under `/peer/*` for LAN collaboration:

- `GET /peer/projects`
- `GET /peer/sessions`
- `GET /peer/sessions/{sessionId}`
- `GET /peer/sessions/{sessionId}/deltas`
- `GET /peer/streams/session-deltas`

These endpoints require `CSM_PEER_TOKEN`, project share policy allowlisting, a share label such as `share`, `team`, `review`, or `collab`, redaction, and response length limits. They return summaries, excerpts, deltas, paths, commands, and git refs; they do not return full JSONL session files.

The archive-delete endpoint copies the source file into the configured local archive directory, records a checksum, and only then marks the session as deleted in manager metadata. The archive provider is intentionally isolated behind this API so an S3 provider can replace the local copy implementation later.

The activity summary endpoint filters non-deleted sessions modified within the requested time window, compresses the session data, and calls `codex exec --ephemeral --sandbox read-only` to generate a Markdown report. The UI currently offers 1, 7, 14, 30, and 90 day ranges.

The backend also stores the last successfully scanned workspace path in metadata. On startup it attempts a best-effort rescan of that path so labels, notes, deleted status, and the visible session list survive a backend restart when the workspace is still available.

## Verify

```bash
cd backend
cargo fmt --check
cargo check

cd ../frontend
npm run lint
npm run build

cd src-tauri
cargo fmt --check
cargo check

cd ..
npm run tauri:build
```
