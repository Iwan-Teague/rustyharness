#!/bin/sh
# Purity + INV-28 gates (rustyharness design docs/01-design-v0.1.md §1.2).
#
# Four pure crates: gate-outcome, harness-core, harness-manifest and
# harness-policy. Purity means no I/O, no async, no clock reads, no global
# state — checked two ways:
#
#   1. dependency shape: `cargo tree -e normal` of each pure crate must stay
#      inside its reviewed allowlist (gate-outcome has ZERO default deps);
#   2. content: the pure crates' Rust files must not so much as NAME an I/O,
#      process, environment, thread, console, or clock facility.
#
# INV-28: exactly one outcome type in the suite. No `enum …Outcome` /
# `enum …Verdict` may appear in any Rust file under crates/harness-* — the
# verdict lives in gate-outcome (gate-outcome itself is exempt: it IS the
# outcome crate).
#
# Fail closed: every tool call is checked. A tool that errors, or a check
# that read nothing, fails the gate; it never prints OK.
# scripts/ci/purity-selftest.sh plants violations and proves each refusal.
set -eu

cd "$(dirname "$0")/../.."

fail() {
    printf 'purity gate FAILED: %s\n' "$1" >&2
    exit 1
}

tmpdir=$(mktemp -d) || fail "mktemp failed"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

# --- 1. dependency shape ---------------------------------------------------

# tree_names OUT CRATE [ARGS...]: the sorted crate names in CRATE's normal
# dependency tree, written to OUT. No pipelines: POSIX sh has no pipefail,
# so each step writes a file and its exit status is checked.
tree_names() {
    out=$1
    crate=$2
    shift 2
    cargo tree -e normal --prefix none -p "$crate" "$@" >"$tmpdir/tree-raw" ||
        fail "cargo tree failed: -p $crate $*"
    awk '{print $1}' "$tmpdir/tree-raw" >"$tmpdir/tree-names" ||
        fail "awk failed on cargo tree output for $crate"
    sort -u "$tmpdir/tree-names" >"$out" ||
        fail "sort failed on cargo tree output for $crate"
    # Content, not presence: the tree must at least contain the crate itself.
    grep -qxF "$crate" "$out" ||
        fail "cargo tree output for $crate does not name $crate (read nothing?)"
}

# refuse_intruders TREE ALLOWED WHAT: fail if TREE names a crate not in ALLOWED.
refuse_intruders() {
    comm -23 "$1" "$2" >"$tmpdir/intruders" || fail "comm failed ($3)"
    if [ -s "$tmpdir/intruders" ]; then
        fail "$3 pulled in non-allowlisted crates:
$(cat "$tmpdir/intruders")"
    fi
}

# gate-outcome is the bottom of the crate lattice: ZERO normal dependencies
# on default features.
tree_names "$tmpdir/gate-default" gate-outcome
printf '%s\n' gate-outcome >"$tmpdir/allowed-default"
refuse_intruders "$tmpdir/gate-default" "$tmpdir/allowed-default" "gate-outcome (default)"

# With the json feature (§6 child protocol) only the reviewed serde stack is
# allowed. serde_derive's proc-macro deps appear here when a feature turns
# them on; the allowlist admits the whole reviewed stack, not just today's
# shape, so a serde build-detail change cannot silently re-fail the gate
# (memchr and zmij arrive with serde_json >= 1.0.14x: both pure Rust, no
# links, no cc; zmij's build.rs only probes the rustc version) — but ANY
# OTHER crate still fails it.
tree_names "$tmpdir/gate-json" gate-outcome --features json
grep -qxF serde_json "$tmpdir/gate-json" ||
    fail "gate-outcome --features json tree does not contain serde_json (feature renamed?)"
printf '%s\n' \
    gate-outcome serde serde_core serde_derive proc-macro2 quote syn \
    unicode-ident serde_json itoa ryu memchr zmij >"$tmpdir/allowed-json-raw"
sort -u "$tmpdir/allowed-json-raw" >"$tmpdir/allowed-json" || fail "sort failed"
refuse_intruders "$tmpdir/gate-json" "$tmpdir/allowed-json" "gate-outcome (json)"

# harness-core may depend only on gate-outcome (design §1.2). serde and
# serde_json stay admitted for the child-protocol wire types; any new
# dependency must be added to this allowlist BY REVIEW.
tree_names "$tmpdir/core" harness-core
grep -qxF gate-outcome "$tmpdir/core" ||
    fail "harness-core tree does not contain gate-outcome (read the wrong tree?)"
printf '%s\n' \
    harness-core gate-outcome serde serde_core serde_derive proc-macro2 \
    quote syn unicode-ident serde_json itoa ryu >"$tmpdir/allowed-core-raw"
sort -u "$tmpdir/allowed-core-raw" >"$tmpdir/allowed-core" || fail "sort failed"
refuse_intruders "$tmpdir/core" "$tmpdir/allowed-core" "harness-core"

# harness-manifest and harness-policy (design §1.2): serde, serde_json,
# thiserror (+ its proc-macro), and policy -> manifest. No SHA-256 or
# ed25519 crate yet (pinning and signing are H4); adding one is a review
# decision recorded here with its reason.
#   serde stack (serde, serde_core, serde_derive, proc-macro2, quote, syn,
#     unicode-ident, serde_json, itoa, ryu, memchr, zmij): as above;
#   thiserror, thiserror-impl: derive-only error Display, no runtime code.
tree_names "$tmpdir/manifest" harness-manifest
grep -qxF serde_json "$tmpdir/manifest" ||
    fail "harness-manifest tree does not contain serde_json (read the wrong tree?)"
printf '%s\n' \
    harness-manifest serde serde_core serde_derive proc-macro2 quote syn \
    unicode-ident serde_json itoa ryu memchr zmij thiserror thiserror-impl \
    >"$tmpdir/allowed-manifest-raw"
sort -u "$tmpdir/allowed-manifest-raw" >"$tmpdir/allowed-manifest" || fail "sort failed"
refuse_intruders "$tmpdir/manifest" "$tmpdir/allowed-manifest" "harness-manifest"

tree_names "$tmpdir/policy" harness-policy
grep -qxF harness-manifest "$tmpdir/policy" ||
    fail "harness-policy tree does not contain harness-manifest (read the wrong tree?)"
printf '%s\n' harness-policy >>"$tmpdir/allowed-manifest-raw" || fail "printf failed"
sort -u "$tmpdir/allowed-manifest-raw" >"$tmpdir/allowed-policy" || fail "sort failed"
refuse_intruders "$tmpdir/policy" "$tmpdir/allowed-policy" "harness-policy"

# --- shared: file lists and normalisation -----------------------------------

# rust_files OUT DIR...: every .rs file under DIR... (src, tests, benches,
# examples, build scripts), failing on a find error or an empty result.
rust_files() {
    out=$1
    shift
    find "$@" -type f -name '*.rs' >"$tmpdir/found" || fail "find $* failed"
    sort "$tmpdir/found" >"$out" || fail "sort failed"
    [ -s "$out" ] || fail "no Rust files found under $*"
}

# normalise FILE OUT: the file as ONE line, every run of whitespace (space,
# tab, CR, newline) collapsed to one space and the spaces around `::`
# removed, so `use std :: fs`, `use std::{\n fs,\n}` and `enum<TAB>X` all
# read like their one-line forms.
normalise() {
    awk '{ printf "%s ", $0 } END { printf "\n" }' "$1" >"$tmpdir/joined" ||
        fail "awk could not read $1"
    awk '{ gsub(/[ \t\r]+/, " "); gsub(/ ?:: ?/, "::"); print }' \
        "$tmpdir/joined" >"$2" || fail "awk could not normalise $1"
}

# scan WHAT PATTERN NORMALISED FILE: record a hit if the ERE matches.
# grep's exit status is read directly: 0 = hit, 1 = clean, anything else =
# an error, which fails the gate.
scan() {
    rc=0
    grep -oE "$2" "$3" >"$tmpdir/match" || rc=$?
    case $rc in
        0) while IFS= read -r m; do
               printf '%s: %s: %s\n' "$4" "$1" "$m" >>"$tmpdir/hits"
           done <"$tmpdir/match" ;;
        1) ;;
        *) fail "grep error (rc=$rc) scanning $4" ;;
    esac
}

# Identifier boundary (grep -E has no portable \b).
nb='[^A-Za-z0-9_]'

# --- 2. content ------------------------------------------------------------

# Refuse I/O, process, environment, thread, console, and clock facilities BY
# NAME in the pure crates. (std::time::Duration is fine: it is a length of
# time, not a clock read — time is passed IN, design §2.4.)
# `path` is in the list (review F-2): std::path::Path does filesystem I/O
# through methods (exists, canonicalize, read_dir, ...) that never name
# std::fs, so the pure crates may not import std::path at all.
facility='(fs|net|process|env|io|os|thread|path)'
rust_files "$tmpdir/pure-files" crates/gate-outcome crates/harness-core \
    crates/harness-manifest crates/harness-policy
for must in crates/gate-outcome/src/lib.rs crates/harness-core/src/lib.rs \
    crates/harness-manifest/src/lib.rs crates/harness-policy/src/lib.rs; do
    grep -qxF "$must" "$tmpdir/pure-files" || fail "pure-content scan would miss $must"
done
: >"$tmpdir/hits"
while IFS= read -r f; do
    normalise "$f" "$tmpdir/norm"
    # std::fs, ::std::io::stdout, std :: net ...
    scan "std path" "(^|$nb)std::$facility($nb|\$)" "$tmpdir/norm" "$f"
    # use std::{collections::HashMap, fs}; use std::{fs as f, net as n};
    # (anything inside a std::{…} group up to the end of the use item)
    scan "std group import" "(^|$nb)std::\{([^;]*[^A-Za-z0-9_;])?$facility($nb|\$)" \
        "$tmpdir/norm" "$f"
    # use std as s; use std::{self as s}; extern crate std as s;
    scan "renamed std" "(^|$nb)std as($nb|\$)" "$tmpdir/norm" "$f"
    scan "renamed std" "(^|$nb)std::\{([^;]*[^A-Za-z0-9_;])?self($nb|\$)" "$tmpdir/norm" "$f"
    scan "clock" "(^|$nb)(SystemTime|Instant|UNIX_EPOCH)($nb|\$)" "$tmpdir/norm" "$f"
    # Path/PathBuf I/O methods, however the receiver was obtained (belt and
    # braces for the `path` facility above).
    scan "path I/O method" "\.(exists|try_exists|metadata|symlink_metadata|canonicalize|read_dir|read_link|is_file|is_dir|is_symlink) ?\(" "$tmpdir/norm" "$f"
    scan "console" "(^|$nb)(print|println|eprint|eprintln|dbg)!" "$tmpdir/norm" "$f"
    scan "thread" "(^|$nb)thread::spawn($nb|\$)" "$tmpdir/norm" "$f"
    scan "async" "(^|$nb)async ?(fn|move|\{)" "$tmpdir/norm" "$f"
    scan "runtime crate" "(^|$nb)(tokio|mio|async_std)($nb|\$)" "$tmpdir/norm" "$f"
done <"$tmpdir/pure-files"
if [ -s "$tmpdir/hits" ]; then
    fail "pure sources name forbidden facilities:
$(cat "$tmpdir/hits")"
fi

# --- 3. INV-28: exactly one outcome type -----------------------------------

rust_files "$tmpdir/harness-files" crates/harness-*
grep -qxF crates/harness-core/src/lib.rs "$tmpdir/harness-files" ||
    fail "INV-28 scan would miss crates/harness-core/src/lib.rs"
: >"$tmpdir/hits"
while IFS= read -r f; do
    normalise "$f" "$tmpdir/norm"
    # Prefix match on purpose: `enum RunOutcomeKind` is refused too.
    scan "INV-28" "(^|$nb)enum [A-Za-z0-9_]*(Outcome|Verdict)" "$tmpdir/norm" "$f"
done <"$tmpdir/harness-files"
if [ -s "$tmpdir/hits" ]; then
    fail "INV-28: verdict enums must live in gate-outcome, not harness-*:
$(cat "$tmpdir/hits")"
fi

printf 'purity gate OK: dependency shape, pure-content, INV-28 all clean.\n'
