//! `rustyharness` — SCAFFOLD (2026-09-23). Only non-executing commands exist.

#![forbid(unsafe_code)]

use std::process::ExitCode;

const USAGE: &str = "usage:
  rustyharness version
  rustyharness sandbox             report confinement (refuses to run anything without it)
  rustyharness manifest check <file.json>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["version"] => {
            println!("rustyharness {} (scaffold)", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        ["sandbox"] => match harness_sandbox::require() {
            Ok(b) => {
                println!("confinement available: {b:?}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::from(3)
            }
        },
        ["manifest", "check", path] => manifest_check(path),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn manifest_check(path: &str) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let manifest: harness_tools::Manifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("REFUSED {path}: does not parse as a manifest: {e}");
            return ExitCode::FAILURE;
        }
    };
    match manifest.validate() {
        Ok(()) => {
            println!(
                "OK {path}: {} {} — {} capabilit(y/ies)",
                manifest.app,
                manifest.app_version,
                manifest.capabilities.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("REFUSED {path}: {e}");
            ExitCode::FAILURE
        }
    }
}
