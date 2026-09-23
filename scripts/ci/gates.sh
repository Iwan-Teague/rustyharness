#!/bin/sh
# rustyharness member gate entrypoint (same contract as the other members, AQ-148).
#
# tooling/dev-push.sh runs exactly this file and advances `development` only on
# exit 0. "No gates" is not "gates passed"; a missing tool fails the run rather
# than skipping it. Licences are gated: this project's licence is decided
# (docs/adr/0003-licence.md), unlike the suite-wide ruling (OI-05).
set -eu

cd "$(dirname "$0")/../.."

printf '=== 1/4 cargo fmt --all --check ===\n'
cargo fmt --all --check

printf '=== 2/4 cargo deny check advisories bans licenses sources ===\n'
cargo deny check advisories bans licenses sources

printf '=== 3/4 cargo clippy --workspace --all-targets -- -D warnings ===\n'
cargo clippy --locked --workspace --all-targets -- -D warnings

printf '=== 4/4 cargo test --workspace ===\n'
cargo test --locked --workspace

printf 'All 4 rustyharness gates passed.\n'
