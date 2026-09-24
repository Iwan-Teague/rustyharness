#!/bin/sh
# Refusal witnesses for scripts/ci/purity.sh (design §10: INV-28's
# falsifying test is "the build fails on a planted one").
#
# Each case copies the source tree to a temporary directory, plants ONE
# violation (or breaks one tool), runs the copy's purity.sh, and requires a
# non-zero exit WITH the expected reason. A clean copy must pass first, so a
# purity.sh that refuses everything cannot satisfy this script. The real
# tree is never modified.
set -eu

cd "$(dirname "$0")/../.."

fail() {
    printf 'purity selftest FAILED: %s\n' "$1" >&2
    exit 1
}

tmpdir=$(mktemp -d) || fail "mktemp failed"
trap 'rm -rf "$tmpdir"' EXIT INT TERM

real_cargo=$(command -v cargo) || fail "cargo not found on PATH"

# One source snapshot, without VCS metadata or build output.
COPYFILE_DISABLE=1 tar -cf "$tmpdir/src.tar" --exclude=./.git --exclude=./target . ||
    fail "could not snapshot the source tree"

n=0
# fresh: extract a clean copy; prints nothing, sets $copy.
fresh() {
    n=$((n + 1))
    copy="$tmpdir/case$n"
    mkdir "$copy" || fail "mkdir $copy failed"
    tar -xf "$tmpdir/src.tar" -C "$copy" || fail "could not extract the snapshot"
    [ -f "$copy/scripts/ci/purity.sh" ] && [ -f "$copy/crates/harness-core/src/lib.rs" ] ||
        fail "snapshot is missing files (read nothing?)"
}

# run_purity [ENV...]: run the copy's purity.sh; sets $rc, output in $tmpdir/out.
run_purity() {
    rc=0
    env "$@" sh "$copy/scripts/ci/purity.sh" >"$tmpdir/out" 2>&1 || rc=$?
}

# expect_refusal NAME WANT [ENV...]: purity.sh must exit non-zero and say WANT.
expect_refusal() {
    name=$1
    want=$2
    shift 2
    run_purity "$@"
    [ "$rc" -ne 0 ] || fail "$name: purity.sh ACCEPTED the planted violation:
$(cat "$tmpdir/out")"
    grep -qF -- "$want" "$tmpdir/out" || fail "$name: refused for the wrong reason (wanted '$want'):
$(cat "$tmpdir/out")"
    printf 'ok refused: %s\n' "$name"
}

# plant REL-PATH CONTENT: create a file in the current copy.
plant() {
    mkdir -p "$(dirname "$copy/$1")" || fail "mkdir for $1 failed"
    printf '%b' "$2" >"$copy/$1" || fail "could not plant $1"
}

# --- control: the clean tree passes ------------------------------------------
fresh
run_purity
[ "$rc" -eq 0 ] || fail "clean copy does not pass purity.sh (rc=$rc):
$(cat "$tmpdir/out")"
printf 'ok accepted: clean tree\n'

# --- pure-content plants (harness-core, a new uncompiled file) ---------------
content_case() {
    fresh
    plant crates/harness-core/src/zz_plant.rs "$2"
    expect_refusal "$1" "pure sources name forbidden facilities"
}
content_case "use std::fs" 'use std::fs;\n'
content_case "brace group with fs" 'use std::{collections::HashMap, fs};\n'
content_case "renamed group" 'use std::{fs as f, net as n, process as p, env as e};\n'
content_case "multi-line group" 'use std::{\n    collections::HashMap,\n    fs,\n};\n'
content_case "spaced path" 'use std :: fs;\n'
content_case "std::io stdout" 'fn f() { let _ = std::io::stdout(); }\n'
content_case "println" 'fn f() { println!("x"); }\n'
content_case "use std as" 'use std as s;\n'
content_case "std self rename" 'use std::{self as s};\n'
content_case "clock read" 'fn f() { let _ = std::time::Instant::now(); }\n'
content_case "async fn" 'async fn f() {}\n'
fresh
plant crates/gate-outcome/tests/zz_plant.rs 'use std::{env, fmt};\n'
expect_refusal "gate-outcome integration test uses env" "pure sources name forbidden facilities"
# The two H1b pure crates are scanned too (a plant in each must be refused).
fresh
plant crates/harness-manifest/src/zz_plant.rs 'fn f() { let _ = std::fs::read("m.json"); }\n'
expect_refusal "harness-manifest reads a file" "pure sources name forbidden facilities"
fresh
plant crates/harness-policy/src/zz_plant.rs 'use std::time::SystemTime;\n'
expect_refusal "harness-policy reads the clock" "pure sources name forbidden facilities"
# Review F-2: filesystem I/O through std::path methods never names std::fs.
fresh
plant crates/harness-policy/src/zz_plant.rs 'use std::path::Path;\npub(crate) fn zz(p: &Path) -> bool { p.exists() || p.canonicalize().is_ok() || p.read_dir().is_ok() }\n'
expect_refusal "harness-policy does I/O through Path methods" "pure sources name forbidden facilities"
fresh
plant crates/harness-manifest/src/zz_plant.rs 'fn zz(p: &str) -> bool { let q = std::path::PathBuf::from(p); q.is_file() }\n'
expect_refusal "harness-manifest does I/O through PathBuf::is_file" "pure sources name forbidden facilities"
fresh
plant crates/harness-core/src/zz_plant.rs 'fn zz(p: &::std::path::Path) -> bool { p.try_exists().is_ok() || p.symlink_metadata().is_ok() || p.read_link().is_ok() || p.metadata().is_ok() || p.is_dir() }\n'
expect_refusal "harness-core does I/O through Path methods" "pure sources name forbidden facilities"
# The method scan alone (the receiver's type is never named here).
fresh
plant crates/harness-policy/src/zz_plant.rs 'fn zz<P: Sized>(p: P, f: impl Fn(&P) -> bool) -> bool { f(&p) }\nfn yy(q: &Q) -> bool { q.canonicalize ().is_ok() }\n'
expect_refusal "Path I/O method on an unnamed receiver type" "path I/O method"

# --- INV-28 plants ------------------------------------------------------------
inv28_case() {
    fresh
    plant "$2" "$3"
    expect_refusal "$1" "INV-28"
}
inv28_case "enum RunOutcome" crates/harness-core/src/zz_plant.rs 'pub enum RunOutcome { A }\n'
inv28_case "enum<TAB>RunOutcome" crates/harness-core/src/zz_plant.rs 'pub enum\tRunOutcome { A }\n'
inv28_case "enum<NL>RunVerdict" crates/harness-core/src/zz_plant.rs 'pub enum\nRunVerdict { A }\n'
inv28_case "enum in benches/" crates/harness-core/benches/zz_plant.rs 'enum RunOutcome { A }\n'
inv28_case "enum in another harness crate" crates/harness-tools/src/zz_plant.rs 'enum ToolVerdict { A }\n'
inv28_case "enum in harness-policy" crates/harness-policy/src/zz_plant.rs 'pub enum PolicyOutcome { Allow }\n'

# --- dependency plant ---------------------------------------------------------
# The planted edge is to harness-sandbox, an I/O crate that depends on none of
# the pure crates (harness-tools now depends on harness-core and
# harness-policy, so planting it would make a cycle, not an intruder).
fresh
awk '{ print } /^\[dependencies\]/ { print "harness-sandbox = { path = \"../harness-sandbox\" }" }' \
    "$copy/crates/harness-core/Cargo.toml" >"$tmpdir/Cargo.toml.planted" ||
    fail "awk failed planting a dependency"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-core/Cargo.toml" || fail "mv failed"
grep -qF 'harness-sandbox = { path' "$copy/crates/harness-core/Cargo.toml" ||
    fail "dependency plant did not land"
expect_refusal "harness-core depends on harness-sandbox" "harness-core pulled in non-allowlisted crates"

fresh
awk '{ print } /^\[dependencies\]/ { print "harness-sandbox = { path = \"../harness-sandbox\" }" }' \
    "$copy/crates/harness-policy/Cargo.toml" >"$tmpdir/Cargo.toml.planted" ||
    fail "awk failed planting a dependency"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-policy/Cargo.toml" || fail "mv failed"
grep -qF 'harness-sandbox = { path' "$copy/crates/harness-policy/Cargo.toml" ||
    fail "dependency plant did not land"
expect_refusal "harness-policy depends on harness-sandbox" "harness-policy pulled in non-allowlisted crates"

# The test-only journal seam must not be enabled by a normal dependency.
fresh
awk '{ print } /^\[dependencies\]/ { print "harness-journal = { path = \"../harness-journal\", features = [\"fault-injection\"] }" }' \
    "$copy/crates/harness-cli/Cargo.toml" >"$tmpdir/Cargo.toml.planted" ||
    fail "awk failed planting a dependency"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-cli/Cargo.toml" || fail "mv failed"
grep -qF 'features = ["fault-injection"]' "$copy/crates/harness-cli/Cargo.toml" ||
    fail "fault-injection plant did not land"
expect_refusal "harness-cli enables the journal's fault-injection seam" \
    "a normal dependency edge enables harness-journal/fault-injection"

# INV-24: a TLS crate in the default build is refused (planted as a local
# crate named `rustls`, depended on by harness-model).
fresh
mkdir -p "$copy/crates/zz-rustls/src" || fail "mkdir failed"
printf '[package]\nname = "rustls"\nversion = "0.0.0"\nedition = "2021"\npublish = false\nlicense = "MIT"\n' \
    >"$copy/crates/zz-rustls/Cargo.toml" || fail "could not plant rustls"
printf '' >"$copy/crates/zz-rustls/src/lib.rs" || fail "could not plant rustls lib"
awk '{ print } /^\[dependencies\]/ { print "rustls = { path = \"../zz-rustls\" }" }' \
    "$copy/crates/harness-model/Cargo.toml" >"$tmpdir/Cargo.toml.planted" ||
    fail "awk failed planting a dependency"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-model/Cargo.toml" || fail "mv failed"
expect_refusal "harness-model depends on a TLS crate" "INV-24: TLS/HTTP-client crates in the default build"

# A pure model file naming an I/O facility is refused like a pure crate.
fresh
printf 'fn zz() { let _ = std::net::TcpStream::connect("x"); }\n' >>"$copy/crates/harness-model/src/wire.rs" ||
    fail "could not plant into wire.rs"
expect_refusal "harness-model's pure wire.rs opens a socket" "pure sources name forbidden facilities"

# --- H1e-1 plants --------------------------------------------------------------

# H1d review F-3: a pure model file reaching a sibling I/O module.
fresh
printf 'fn zz() { let _ = crate::http::exchange; }\n' >>"$copy/crates/harness-model/src/wire.rs" ||
    fail "could not plant into wire.rs"
expect_refusal "pure wire.rs names crate::http" "a pure harness-model file reaches a non-pure module"
fresh
printf 'use crate::{profile::Profile as _P, client::OpenAiCompatible as _C};\n' \
    >>"$copy/crates/harness-model/src/protocol.rs" || fail "could not plant into protocol.rs"
expect_refusal "pure protocol.rs imports client in a use group" "a pure harness-model file reaches a non-pure module"
fresh
printf 'pub mod zz;\n' >>"$copy/crates/harness-model/src/lib.rs" || fail "could not plant a module"
printf '' >"$copy/crates/harness-model/src/zz.rs" || fail "could not plant zz.rs"
expect_refusal "an unclassified harness-model module" "is not classified as pure or I/O"

# H1d review F-4: any crate outside harness-model's allowlist, whatever its name.
fresh
mkdir -p "$copy/crates/zz-extra/src" || fail "mkdir failed"
printf '[package]\nname = "zz-innocuous"\nversion = "0.0.0"\nedition = "2021"\npublish = false\nlicense = "MIT"\n' \
    >"$copy/crates/zz-extra/Cargo.toml" || fail "could not plant zz-innocuous"
printf '' >"$copy/crates/zz-extra/src/lib.rs" || fail "could not plant zz-innocuous lib"
awk '{ print } /^\[dependencies\]/ { print "zz-innocuous = { path = \"../zz-extra\" }" }' \
    "$copy/crates/harness-model/Cargo.toml" >"$tmpdir/Cargo.toml.planted" ||
    fail "awk failed planting a dependency"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-model/Cargo.toml" || fail "mv failed"
expect_refusal "harness-model depends on a crate outside its allowlist" "harness-model pulled in non-allowlisted crates"

# H1c confirming review NF-1: a workspace feature that forwards to fault-injection.
fresh
awk '{ print } /^\[dependencies\]/ { print "harness-journal = { path = \"../harness-journal\" }" }' \
    "$copy/crates/harness-cli/Cargo.toml" >"$tmpdir/Cargo.toml.planted" || fail "awk failed"
mv "$tmpdir/Cargo.toml.planted" "$copy/crates/harness-cli/Cargo.toml" || fail "mv failed"
printf '\n[features]\nchaos = ["harness-journal/fault-injection"]\n' >>"$copy/crates/harness-cli/Cargo.toml" ||
    fail "could not plant the chaos feature"
expect_refusal "a chaos feature forwards to fault-injection" "a workspace feature forwards to harness-journal/fault-injection"
# ... and the compile_error! that keeps it out of optimised builds.
fresh
awk '!/^compile_error!\(/ && !/test-only and refused in optimised builds"$/ && !/^\);$/' \
    "$copy/crates/harness-journal/src/lib.rs" >"$tmpdir/lib.rs.planted" || fail "awk failed"
mv "$tmpdir/lib.rs.planted" "$copy/crates/harness-journal/src/lib.rs" || fail "mv failed"
grep -q 'compile_error' "$copy/crates/harness-journal/src/lib.rs" &&
    fail "compile_error! plant did not land"
expect_refusal "the fault-injection compile_error! removed" "an optimised build with harness-journal/fault-injection compiles"

# H1a review N-3: the remaining name-scan gaps.
n3_case() {
    fresh
    plant crates/harness-core/src/zz_plant.rs "$2"
    expect_refusal "$1" "pure sources name forbidden facilities"
}
n3_case "a comment between path segments" 'fn f() { let _ = std::/*x*/fs::read("a"); }\n'
n3_case "#[path] module" '#[path = "../../harness-journal/src/writer.rs"]\nmod w;\n'
n3_case "include_str!" 'const X: &str = include_str!("lib.rs");\n'
n3_case "env!" 'const X: &str = env!("HOME");\n'
n3_case "macro_rules!" 'macro_rules! m { ($a:ident) => { std::$a::read("a") } }\n'
n3_case "static global" 'static COUNTER: u32 = 0;\n'
fresh
plant crates/harness-core/src/zz_plant.rs 'pub enum /*c*/ RunOutcome { A }\n'
expect_refusal "INV-28 with a comment inside" "INV-28"

# H1c review F-6: TrustedName implemented outside the owning files.
fresh
plant crates/harness-tools/src/zz_plant.rs 'struct S(String);\nimpl harness_core::TrustedName for S { fn trusted_name(&self) -> &str { &self.0 } }\n'
expect_refusal "TrustedName vouched for by an unlisted file" "TrustedName implemented outside the files that own"

# H1a review N-6: a compile_fail doctest without its error code.
fresh
printf '/// ```compile_fail\n/// let x: u8 = "a";\n/// ```\npub fn zz() {}\n' >>"$copy/crates/harness-tools/src/lib.rs" ||
    fail "could not plant a doctest"
expect_refusal "compile_fail without an error code" "compile_fail doctests without an expected error code"

# --- tool failures must fail closed -------------------------------------------
mkdir "$tmpdir/shim" || fail "mkdir shim failed"
cat >"$tmpdir/shim/cargo" <<EOF
#!/bin/sh
# Fails 'cargo tree' when an argument equals \$SHIM_FAIL_ARG; with
# SHIM_EMPTY=1 it prints nothing and succeeds. Everything else is real cargo.
if [ "\$1" = tree ]; then
    for a in "\$@"; do
        if [ "\$a" = "\${SHIM_FAIL_ARG:-}" ]; then
            echo "error: simulated cargo tree failure" >&2
            exit 101
        fi
    done
    [ "\${SHIM_EMPTY:-0}" = 1 ] && exit 0
fi
exec "$real_cargo" "\$@"
EOF
chmod +x "$tmpdir/shim/cargo" || fail "chmod shim failed"
shim_path="$tmpdir/shim:$PATH"

fresh
expect_refusal "cargo tree fails (default tree)" "cargo tree failed: -p gate-outcome" \
    PATH="$shim_path" SHIM_FAIL_ARG=gate-outcome
fresh
expect_refusal "cargo tree fails (json tree only)" "cargo tree failed: -p gate-outcome --features json" \
    PATH="$shim_path" SHIM_FAIL_ARG=json
fresh
expect_refusal "cargo tree fails (harness-core tree only)" "cargo tree failed: -p harness-core" \
    PATH="$shim_path" SHIM_FAIL_ARG=harness-core
fresh
expect_refusal "cargo tree fails (harness-manifest tree only)" "cargo tree failed: -p harness-manifest" \
    PATH="$shim_path" SHIM_FAIL_ARG=harness-manifest
fresh
expect_refusal "cargo tree fails (harness-policy tree only)" "cargo tree failed: -p harness-policy" \
    PATH="$shim_path" SHIM_FAIL_ARG=harness-policy
fresh
expect_refusal "cargo tree prints nothing" "read nothing" \
    PATH="$shim_path" SHIM_EMPTY=1

# An unreadable pure source must fail, not be skipped (not testable as root,
# who can read any file).
if [ "$(id -u)" != 0 ]; then
    fresh
    chmod 000 "$copy/crates/harness-core/src/lib.rs" || fail "chmod failed"
    expect_refusal "unreadable pure source" "could not read"
    chmod 644 "$copy/crates/harness-core/src/lib.rs" || fail "chmod restore failed"
fi

printf 'purity selftest OK: %s cases refused or accepted as required.\n' "$n"
