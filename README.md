# rustyharness

A standalone **agent harness**: the runtime that drives a language model through
tools to do real work, with confinement that fails closed and results decided by
evidence, never by the agent's own say-so. Bring your own agent and model; nothing
else is required.

Built alongside [rustysuite](https://github.com/Iwan-Teague), whose capabilities —
developing and reviewing its code, operating its apps, assistants inside them —
ship as optional add-ons, off unless enabled ([ADR-0002](docs/adr/0002-standalone-first.md)).

The model is a swappable part. The harness is what makes an agent reliable.

## Status

**Scaffold — design phase (2026-09-23).** Nothing executes yet. The crates encode
only the rules already settled by reviewed suite designs:

- **No sandbox, no execution.** `rustyharness sandbox` refuses on every platform until a backend passes conformance.
- **Tool output is data.** Everything from outside the trust boundary is `Untrusted`, with no `Deref` and no content in `Debug`.
- **Apps plug in through versioned capability manifests.** Unknown fields, schema versions and namespaces are refused.
- **No private outcome type.** Run results will use the suite's single gate-outcome type.

## Layout

```
crates/
  harness-core      Untrusted<T>, budgets                        (pure, no I/O)
  harness-model     ModelBackend trait — the model is swappable
  harness-tools     capability manifest schema + content validation
  harness-sandbox   fail-closed confinement (no backend yet → refuses)
  harness-journal   append-only run evidence
  harness-cli       the `rustyharness` binary
adapters/           one directory per suite app: its capability manifest
docs/               overview, ADRs, open questions, research pipeline
scripts/ci/gates.sh the member gate entrypoint (fmt, deny, clippy, test)
```

```bash
cargo test --workspace
cargo run -p harness-cli -- manifest check adapters/example/manifest.json
cargo run -p harness-cli -- sandbox
```

## Read order

1. [docs/00-overview.md](docs/00-overview.md) — what it is, where it is used, what it must do, how apps slot in
2. [docs/adr/0001-name-and-placement.md](docs/adr/0001-name-and-placement.md), [docs/adr/0002-standalone-first.md](docs/adr/0002-standalone-first.md)
3. [docs/OPEN-QUESTIONS.md](docs/OPEN-QUESTIONS.md)
4. [docs/research/README.md](docs/research/README.md) — the research pipeline feeding the v0.1 design

Portable by design: CI runs every gate on macOS, Linux and Windows.
