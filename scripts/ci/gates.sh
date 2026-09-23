#!/bin/sh
# rustyharness member gate entrypoint (same contract as the other members, AQ-148).
#
# tooling/dev-push.sh runs exactly this file and advances `development` only on
# exit 0. "No gates" is not "gates passed"; a missing tool fails the run rather
# than skipping it. `cargo deny check licenses` is omitted pending the suite
# licence ruling (OI-05) -- gating on a licence policy the project has not
# chosen would invent a decision.
set -eu

cd "$(dirname "$0")/../.."

printf '=== 1/4 cargo fmt --all --check ===\n'
cargo fmt --all --check

printf '=== 2/4 cargo deny check advisories bans sources ===\n'
cargo deny check advisories bans sources

printf '=== 3/4 cargo clippy --workspace --all-targets -- -D warnings ===\n'
cargo clippy --workspace --all-targets -- -D warnings

printf '=== 4/4 cargo test --workspace ===\n'
cargo test --workspace

printf 'All 4 rustyharness gates passed.\n'
