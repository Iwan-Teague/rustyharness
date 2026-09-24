#!/bin/sh
# rustyharness member gate entrypoint (same contract as the other members, AQ-148).
#
# tooling/dev-push.sh runs exactly this file and advances `development` only on
# exit 0. "No gates" is not "gates passed"; a missing tool fails the run rather
# than skipping it. Licences are gated: this project's licence is decided
# (docs/adr/0003-licence.md), unlike the suite-wide ruling (OI-05).
set -eu

cd "$(dirname "$0")/../.."

printf '=== 1/5 cargo fmt --all --check ===\n'
cargo fmt --all --check

printf '=== 2/5 purity (dep shape, pure-content, INV-28) ===\n'
sh scripts/ci/purity.sh
printf '%s\n' '--- purity refusal witnesses (planted violations must be refused) ---'
sh scripts/ci/purity-selftest.sh

printf '=== 3/5 cargo deny check advisories bans licenses sources ===\n'
cargo deny check advisories bans licenses sources

printf '=== 4/5 cargo clippy --workspace --all-targets -- -D warnings ===\n'
cargo clippy --locked --workspace --all-targets -- -D warnings
printf '%s\n' '--- clippy again with gate-outcome/json (the §6 wire types must compile in CI too) ---'
cargo clippy --locked --workspace --all-targets --features gate-outcome/json -- -D warnings

printf '=== 5/5 cargo test --workspace ===\n'
cargo test --locked --workspace
printf '%s\n' '--- tests again with gate-outcome/json ---'
cargo test --locked --workspace --features gate-outcome/json
printf '%s\n' '--- compile-fail doctests with their expected error codes enforced ---'
# Stable rustdoc accepts `compile_fail,E0451` but checks the code only when
# it believes it is a nightly build; RUSTC_BOOTSTRAP=1 turns that check on
# (H1a review N-6). It enables no unstable feature in the code under test:
# the crates build on stable without it, as every other step shows.
# Its own target directory (H1e-1 review NF-F): dependency build scripts
# (proc-macro2, thiserror) probe RUSTC_BOOTSTRAP and may build nightly code
# paths, which must not mix with the stable cache every other step uses.
RUSTC_BOOTSTRAP=1 CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/rustc-bootstrap-doctests" \
    cargo test --locked --workspace --doc --features gate-outcome/json

printf 'All 5 rustyharness gates passed.\n'
