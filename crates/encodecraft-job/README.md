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
`Sec-Fetch-Site: cross-site`. Leave `output_dir` unset; EncodeCraft suffixes `-2`, `-3`
instead of overwriting.

## Job JSON (EncodeCraft 0.1.0)

`schema` is the integer `1` (not a string). `source` is tagged by lowercase `kind`.
`preset_id` is required. Reply: `{ok, ids[], error?}`.

```json
{"schema":1,"source":{"kind":"effectcraft","project":"/work/spot.ecproj","comp":"Main","mezzanine":"prores","work_area":false},"preset_id":"system.h264-mp4","start_queue":true}
```

## Token discovery (`discover_ipc_token`)

1. `ENCODECRAFT_TOKEN` (trimmed, non-empty)
2. File `ipc-token` in EncodeCraft's data dir, in order:
   `$ENCODECRAFT_HOME`; then the same
   `directories::ProjectDirs::from("dev", "EncodeCraft", "EncodeCraft").data_dir()`
   EncodeCraft uses (Windows `%APPDATA%\EncodeCraft\EncodeCraft\data`, macOS
   `~/Library/Application Support/dev.EncodeCraft.EncodeCraft`, Linux
   `$XDG_DATA_HOME/encodecraft` or `~/.local/share/encodecraft`); then the
   pre-directories-6 Windows folder `%APPDATA%\EncodeCraft\EncodeCraft` as a
   fallback.

Inbox is `<that data dir>/inbox/`. EncodeCraft creates `ipc-token` on first launch.
