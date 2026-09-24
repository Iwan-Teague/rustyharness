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

# harness-core may depend only on gate-outcome and the ONE SHA-256
# implementation (design §1.2, §1.4: `harness_core::sha256`). serde and
# serde_json (with memchr and zmij, as for gate-outcome's json feature) are
# admitted: since H1d harness-core hosts the shared strict JSON reader
# (`harness_core::strict_json`). Any new dependency must be added to this
# allowlist BY REVIEW.
#   sha2 0.11 (RustCrypto, pure Rust, default features off) and its tree:
#     digest, block-buffer, hybrid-array, typenum, crypto-common, cfg-if,
#     cpufeatures (runtime CPU-feature detection for the SHA extensions),
#     libc (FFI declarations only, no C is compiled; cpufeatures uses it on
#     some targets, e.g. aarch64 macOS and Linux). No `cc`, no `asm` feature.
tree_names "$tmpdir/core" harness-core
grep -qxF gate-outcome "$tmpdir/core" ||
    fail "harness-core tree does not contain gate-outcome (read the wrong tree?)"
grep -qxF sha2 "$tmpdir/core" ||
    fail "harness-core tree does not contain sha2 (read the wrong tree?)"
printf '%s\n' \
    harness-core gate-outcome serde serde_core serde_derive proc-macro2 \
    quote syn unicode-ident serde_json itoa ryu memchr zmij \
    sha2 digest block-buffer hybrid-array typenum crypto-common cfg-if \
    cpufeatures libc >"$tmpdir/allowed-core-raw"
sort -u "$tmpdir/allowed-core-raw" >"$tmpdir/allowed-core" || fail "sort failed"
refuse_intruders "$tmpdir/core" "$tmpdir/allowed-core" "harness-core"

# harness-manifest and harness-policy (design §1.2): serde, serde_json,
# thiserror (+ its proc-macro), policy -> manifest, and (since H1d, for the
# shared strict JSON reader harness_core::strict_json) manifest -> core,
# which brings gate-outcome and the reviewed sha2 tree above. No ed25519
# crate yet (signing is H4); adding one is a review decision recorded here
# with its reason.
#   serde stack (serde, serde_core, serde_derive, proc-macro2, quote, syn,
#     unicode-ident, serde_json, itoa, ryu, memchr, zmij): as above;
#   thiserror, thiserror-impl: derive-only error Display, no runtime code;
#   harness-core, gate-outcome and the sha2 tree: harness-core's allowlist.
tree_names "$tmpdir/manifest" harness-manifest
grep -qxF serde_json "$tmpdir/manifest" ||
    fail "harness-manifest tree does not contain serde_json (read the wrong tree?)"
printf '%s\n' \
    harness-manifest serde serde_core serde_derive proc-macro2 quote syn \
    unicode-ident serde_json itoa ryu memchr zmij thiserror thiserror-impl \
    harness-core gate-outcome sha2 digest block-buffer hybrid-array typenum \
    crypto-common cfg-if cpufeatures libc >"$tmpdir/allowed-manifest-raw"
sort -u "$tmpdir/allowed-manifest-raw" >"$tmpdir/allowed-manifest" || fail "sort failed"
refuse_intruders "$tmpdir/manifest" "$tmpdir/allowed-manifest" "harness-manifest"

tree_names "$tmpdir/policy" harness-policy
grep -qxF harness-manifest "$tmpdir/policy" ||
    fail "harness-policy tree does not contain harness-manifest (read the wrong tree?)"
printf '%s\n' harness-policy >>"$tmpdir/allowed-manifest-raw" || fail "printf failed"
sort -u "$tmpdir/allowed-manifest-raw" >"$tmpdir/allowed-policy" || fail "sort failed"
refuse_intruders "$tmpdir/policy" "$tmpdir/allowed-policy" "harness-policy"

# --- INV-24 / H1d review F-4: harness-model's dependency tree is allowlisted --
# harness-model is the crate that talks to the network. Its normal tree may
# contain only the reviewed crates below; anything else (a TLS stack, an HTTP
# client, anything) fails here, whatever its name. The denylist that follows
# stays as a second, clearer message for the well-known TLS names.
tree_names "$tmpdir/model" harness-model
grep -qxF harness-journal "$tmpdir/model" ||
    fail "harness-model tree does not contain harness-journal (read the wrong tree?)"
printf '%s\n' \
    harness-model harness-core harness-journal gate-outcome \
    serde serde_core serde_derive proc-macro2 quote syn unicode-ident \
    serde_json itoa ryu memchr zmij thiserror thiserror-impl \
    sha2 digest block-buffer hybrid-array typenum crypto-common cfg-if \
    cpufeatures libc >"$tmpdir/allowed-model-raw"
sort -u "$tmpdir/allowed-model-raw" >"$tmpdir/allowed-model" || fail "sort failed"

# --- INV-24: no TLS in the default build ------------------------------------
# The default build connects only to loopback over plain HTTP (design §3.2);
# TLS arrives only with a future, off-by-default `hosted` feature. No crate
# of a TLS or HTTP-client stack may appear on a normal edge anywhere in the
# workspace. The tree must name harness-model, so the check reads something.
cargo tree --workspace -e normal --prefix none >"$tmpdir/ws-raw" ||
    fail "cargo tree failed: --workspace -e normal"
awk '{print $1}' "$tmpdir/ws-raw" >"$tmpdir/ws-names" || fail "awk failed on the workspace tree"
grep -qxF harness-model "$tmpdir/ws-names" ||
    fail "the workspace tree does not name harness-model (read nothing?)"
: >"$tmpdir/tls-hits"
for c in rustls rustls-webpki webpki webpki-roots ring aws-lc-rs aws-lc-sys openssl openssl-sys \
    native-tls tokio-rustls hyper-rustls reqwest hyper ureq curl curl-sys; do
    if grep -qxF "$c" "$tmpdir/ws-names"; then
        printf '%s\n' "$c" >>"$tmpdir/tls-hits"
    fi
done
if [ -s "$tmpdir/tls-hits" ]; then
    fail "INV-24: TLS/HTTP-client crates in the default build:
$(cat "$tmpdir/tls-hits")"
fi
refuse_intruders "$tmpdir/model" "$tmpdir/allowed-model" "harness-model"

# --- test-only seams stay out of normal builds (H1c review F-3) -------------
# harness-journal's `fault-injection` feature compiles a JournalFile that
# fails (or succeeds) on demand. The seam is sealed, and only
# [dev-dependencies] may enable the feature: no NORMAL edge anywhere in the
# workspace may. The all-edges tree must name the feature, so a rename
# cannot make this check vacuous.
cargo tree --workspace -e all,features >"$tmpdir/feat-all" ||
    fail "cargo tree failed: --workspace -e all,features"
grep -qF 'harness-journal feature "fault-injection"' "$tmpdir/feat-all" ||
    fail "the fault-injection feature is not in the all-edges tree (renamed? read nothing?)"
cargo tree --workspace -e normal,features >"$tmpdir/feat-normal" ||
    fail "cargo tree failed: --workspace -e normal,features"
grep -q 'harness-journal' "$tmpdir/feat-normal" ||
    fail "the normal feature tree does not name harness-journal (read nothing?)"
if grep -qF 'fault-injection' "$tmpdir/feat-normal"; then
    fail "a normal dependency edge enables harness-journal/fault-injection"
fi
# H1c confirming review NF-1: `cargo tree` shows only ACTIVE features, so a
# workspace feature that merely FORWARDS to fault-injection (e.g.
# `chaos = ["harness-journal/fault-injection"]`) is invisible above. No
# `[features]` table in the workspace may mention it, except the feature's
# own definition in harness-journal.
: >"$tmpdir/fwd-hits"
for toml in crates/*/Cargo.toml; do
    awk -v f="$toml" '
        /^[ \t]*\[/ { sec = $0 }
        sec ~ /^[ \t]*\[features\]/ && /fault-injection/ {
            if (!(f ~ /harness-journal/ && $0 ~ /^[ \t]*fault-injection[ \t]*=[ \t]*\[[ \t]*\][ \t]*$/))
                print f ": " $0
        }' "$toml" >>"$tmpdir/fwd-hits" || fail "awk failed on $toml"
done
if [ -s "$tmpdir/fwd-hits" ]; then
    fail "a workspace feature forwards to harness-journal/fault-injection:
$(cat "$tmpdir/fwd-hits")"
fi

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

# strip_comments FILE OUT: the file with `//` line comments and (non-nested)
# `/* */` block comments removed (H1a review N-3: `std::/*x*/fs` and
# `enum /*c*/ RunOutcome` hid from the scans). A `//` inside a string
# literal also cuts the rest of that line, so every scan runs on BOTH the
# raw and the stripped text: neither form can hide from both.
strip_comments() {
    awk '
    {
        line = $0; out = ""
        while (length(line) > 0) {
            if (inb) {
                i = index(line, "*/")
                if (i == 0) { line = ""; break }
                line = substr(line, i + 2); inb = 0; continue
            }
            a = index(line, "/*"); b = index(line, "//")
            if (b > 0 && (a == 0 || b < a)) { out = out substr(line, 1, b - 1); line = ""; break }
            if (a > 0) { out = out substr(line, 1, a - 1) " "; line = substr(line, a + 2); inb = 1; continue }
            out = out line; line = ""
        }
        print out
    }' "$1" >"$2" || fail "awk could not read $1 (stripping comments)"
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
# harness-model is an I/O crate (design §1.2), but its parse/validate half is
# pure by design (H1d): these four files are scanned like a pure crate.
for f in crates/harness-model/src/endpoint.rs crates/harness-model/src/wire.rs \
    crates/harness-model/src/protocol.rs crates/harness-model/src/profile.rs; do
    [ -f "$f" ] || fail "pure model file $f is missing (renamed? read nothing?)"
    printf '%s\n' "$f" >>"$tmpdir/pure-files" || fail "printf failed"
done
for must in crates/gate-outcome/src/lib.rs crates/harness-core/src/lib.rs \
    crates/harness-manifest/src/lib.rs crates/harness-policy/src/lib.rs; do
    grep -qxF "$must" "$tmpdir/pure-files" || fail "pure-content scan would miss $must"
done
: >"$tmpdir/hits"
while IFS= read -r f; do
  strip_comments "$f" "$tmpdir/stripped"
  normalise "$tmpdir/stripped" "$tmpdir/norm-code"
  for view in raw code; do
    if [ "$view" = raw ]; then normalise "$f" "$tmpdir/norm"; else cp "$tmpdir/norm-code" "$tmpdir/norm" || fail "cp failed"; fi
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
  done
  # Code-only scans (comments stripped, so prose mentioning the words is
  # fine). H1a review N-3: module and file inclusion from elsewhere,
  # compile-time reads, macros that could splice a forbidden path, and
  # global state.
  scan "#[path] module" "#\[ ?path ?=" "$tmpdir/norm-code" "$f"
  scan "compile-time read" "(^|$nb)(include|include_str|include_bytes|env|option_env) ?!" "$tmpdir/norm-code" "$f"
  scan "macro_rules" "(^|$nb)macro_rules ?!" "$tmpdir/norm-code" "$f"
  scan "static item" "(^|[^A-Za-z0-9_'])static (mut )?[A-Za-z_]" "$tmpdir/norm-code" "$f"
done <"$tmpdir/pure-files"
if [ -s "$tmpdir/hits" ]; then
    fail "pure sources name forbidden facilities:
$(cat "$tmpdir/hits")"
fi


# --- 2b. harness-model: pure files cannot reach I/O modules (H1d review F-3) -
# The four pure model files share a crate with the socket code, so an I/O
# path is one `crate::` away. Every module of harness-model must be
# classified here, and the pure files may not name a non-pure one, through
# `crate::`, `super::`, a `use crate::{…}` group, or `extern crate self`.
model_pure='endpoint wire protocol profile'
model_io='http client replay smoke scripted'
grep -oE '^(pub(\(crate\))? )?mod [a-z_]+;' crates/harness-model/src/lib.rs >"$tmpdir/model-mods-raw" ||
    fail "no module declarations found in harness-model/src/lib.rs (read nothing?)"
awk '{ print $NF }' "$tmpdir/model-mods-raw" | tr -d ';' >"$tmpdir/model-mods" ||
    fail "awk failed on the module list"
while IFS= read -r m; do
    case " $model_pure $model_io " in
        *" $m "*) ;;
        *) fail "harness-model module '$m' is not classified as pure or I/O in scripts/ci/purity.sh" ;;
    esac
done <"$tmpdir/model-mods"
io_alt=$(printf '%s' "$model_io" | tr ' ' '|')
: >"$tmpdir/hits"
for f in crates/harness-model/src/endpoint.rs crates/harness-model/src/wire.rs \
    crates/harness-model/src/protocol.rs crates/harness-model/src/profile.rs; do
    for view in raw code; do
        if [ "$view" = raw ]; then normalise "$f" "$tmpdir/norm"; else
            strip_comments "$f" "$tmpdir/stripped"; normalise "$tmpdir/stripped" "$tmpdir/norm"; fi
        scan "I/O sibling module" "(^|$nb)(crate|super|harness_model)::($io_alt)($nb|\$)" "$tmpdir/norm" "$f"
        scan "I/O sibling module (group)" "(^|$nb)(crate|super)::\{([^;]*[^A-Za-z0-9_;])?($io_alt)($nb|\$)" "$tmpdir/norm" "$f"
        scan "extern crate self" "(^|$nb)extern crate self($nb|\$)" "$tmpdir/norm" "$f"
    done
done
if [ -s "$tmpdir/hits" ]; then
    fail "a pure harness-model file reaches a non-pure module:
$(cat "$tmpdir/hits")"
fi

# --- 2c. typed provenance: who may vouch for trusted journal text ------------
# `harness_core::TrustedName` lets a type put text into a TRUSTED journal
# field (H1c review F-6). Implementing it is a provenance claim, so it may
# appear only in the files that own the vouched-for types.
rust_files "$tmpdir/all-files" crates
: >"$tmpdir/hits"
while IFS= read -r f; do
    case "$f" in
        crates/harness-core/src/lib.rs|crates/harness-manifest/src/lib.rs|crates/harness-model/src/lib.rs) continue ;;
    esac
    strip_comments "$f" "$tmpdir/stripped"
    normalise "$tmpdir/stripped" "$tmpdir/norm"
    scan "TrustedName impl" "(^|$nb)impl[^{;]*TrustedName for($nb|\$)" "$tmpdir/norm" "$f"
done <"$tmpdir/all-files"
if [ -s "$tmpdir/hits" ]; then
    fail "TrustedName implemented outside the files that own vouched-for types:
$(cat "$tmpdir/hits")"
fi

# --- 2d. compile-fail doctests pin their reason (H1a review N-6) -------------
# Every compile_fail doctest names its expected error code; gates.sh runs the
# doctests with RUSTC_BOOTSTRAP=1 so rustdoc enforces the codes.
grep -rn '```compile_fail' crates >"$tmpdir/cf" || fail "no compile_fail doctests found (read nothing?)"
grep -vE '```compile_fail,E[0-9]{4}' "$tmpdir/cf" >"$tmpdir/cf-bad" || true
if [ -s "$tmpdir/cf-bad" ]; then
    fail "compile_fail doctests without an expected error code:
$(cat "$tmpdir/cf-bad")"
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
    # And with comments stripped (`enum /*c*/ RunOutcome`, H1a review N-3).
    strip_comments "$f" "$tmpdir/stripped"
    normalise "$tmpdir/stripped" "$tmpdir/norm"
    scan "INV-28" "(^|$nb)enum [A-Za-z0-9_]*(Outcome|Verdict)" "$tmpdir/norm" "$f"
done <"$tmpdir/harness-files"
if [ -s "$tmpdir/hits" ]; then
    fail "INV-28: verdict enums must live in gate-outcome, not harness-*:
$(cat "$tmpdir/hits")"
fi

# --- 4. no optimised build contains the journal's test seams (H1c NF-1) ------
# harness-journal has a compile_error! for `fault-injection` without debug
# assertions. Check that it FIRES (content, not presence of the line) by
# expanding the crate root with rustc directly: macro expansion reports the
# compile_error! before any dependency is resolved, so this needs no build,
# and nothing is cached between runs (a cached `cargo check` could report a
# stale result for a tree copied with old mtimes). Control: with debug
# assertions on (test builds) it must NOT fire.
command -v rustc >/dev/null 2>&1 || fail "rustc not found on PATH"
fi_check() {
    rustc --edition 2021 --crate-type lib --crate-name harness_journal \
        --cfg 'feature="fault-injection"' -C "debug-assertions=$1" --emit=metadata \
        -o "$tmpdir/fi.rmeta" crates/harness-journal/src/lib.rs >"$tmpdir/fi-$1" 2>&1 || true
    grep -c "test-only and refused in optimised builds" "$tmpdir/fi-$1" >"$tmpdir/fi-count" || true
    read -r fi_n <"$tmpdir/fi-count" || fi_n=0
}
fi_check off
[ "${fi_n:-0}" -gt 0 ] ||
    fail "an optimised build with harness-journal/fault-injection compiles (the compile_error! is gone):
$(cat "$tmpdir/fi-off")"
fi_check on
[ "${fi_n:-0}" -eq 0 ] ||
    fail "the fault-injection compile_error! also fires with debug assertions on (tests would break)"

printf 'purity gate OK: dependency shape, pure-content, INV-28 all clean.\n'
