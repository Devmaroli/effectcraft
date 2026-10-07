# encodecraft-job

Shared JSON schema for an EncodeCraft queue job. **serde only** (no HTTP, no filesystem):
EffectCraft, EncodeCraft and any other sender serialize the same `Job` object.

## Provenance

Vendored into this EffectCraft fork from Kapildev Maroli's EncodeCraft repository
(`crates/job` in [cursor.com/codebase/devmaroli/encodecraft](https://cursor.com/codebase/devmaroli/encodecraft)),
licence **MIT OR Apache-2.0**.

This GitHub repo has to build on its own, so the crate is copied here instead of a
`path = "../encodecraft"` dependency. Field names follow EncodeCraft's documented job
format (`docs/job-format.md` in that repo): a localhost `POST /v1/enqueue` body, and
the same JSON dropped into EncodeCraft's inbox when the HTTP server is not up.

If EncodeCraft's crate later adds fields, extra JSON keys still deserialize (unknown
fields are ignored) and new optional fields can be added here with
`#[serde(default)]`.
