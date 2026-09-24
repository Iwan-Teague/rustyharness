//! The `rustyharness` binary: the CLI library with the refusing default
//! locality probe (`NoProbe`: every `state_root` is refused until spike
//! S-F1 lands real per-OS probes, design §2.8, INV-35), real stdout and
//! stderr, and `GATE_OK_FILE` from the environment. Nothing here, in any
//! build, can select another probe (H1e-2b review F-2).

#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::process::ExitCode;

use harness_policy::locality::NoProbe;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr().lock();
    let cx = harness_cli::Cx {
        probe: &NoProbe,
        gate_ok_file: std::env::var_os("GATE_OK_FILE").map(Into::into),
        out: RefCell::new(&mut out),
        err: RefCell::new(&mut err),
    };
    ExitCode::from(harness_cli::main_with(&cx, &args))
}
