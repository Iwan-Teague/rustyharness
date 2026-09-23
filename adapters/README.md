# adapters/

One directory per suite app that offers capabilities to agents. Each holds the
app's capability manifest (schema: `crates/harness-tools`, `SCHEMA_VERSION`)
and, later, its adapter. Adding an app = adding a directory here; the harness
core does not change. See docs/00-overview.md §4.

`example/manifest.json` is a schema illustration only, not a real app.
