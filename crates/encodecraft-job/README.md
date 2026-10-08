# encodecraft-job

Shared JSON schema and IPC constants for an EncodeCraft queue job. Vendored from
EncodeCraft `crates/job` / `docs/job-format.md` (MIT OR Apache-2.0) so this GitHub
repo builds without a sibling checkout.

## HTTP

| Method | Auth |
|---|---|
| `GET /health` | none; `{"ok":true,"product":"EncodeCraft"}` |
| `POST /v1/enqueue` | header `X-EncodeCraft-Token: <token>` |
| `GET /v1/queue` | same header |
| `POST /v1/control` | same header (`ControlRequest.token` optional in the JSON) |

JSON-lines control channel: each `{id, method, params, token}` line carries `token`.
Loopback only. Do not send a non-loopback `Origin` (including `null`) or
`Sec-Fetch-Site: cross-site`. Leave `outputDir` unset; EncodeCraft suffixes `-2`, `-3`
instead of overwriting.

## Token discovery (`discover_ipc_token`)

1. `ENCODECRAFT_TOKEN` (trimmed, non-empty)
2. File `ipc-token` in, in order: `$ENCODECRAFT_HOME`; Windows
   `%APPDATA%\EncodeCraft\EncodeCraft`; macOS
   `~/Library/Application Support/dev.EncodeCraft.EncodeCraft`; elsewhere
   `$XDG_DATA_HOME/encodecraft` then `~/.local/share/encodecraft`

Inbox is `<that data dir>/inbox/`. EncodeCraft creates `ipc-token` on first launch.
