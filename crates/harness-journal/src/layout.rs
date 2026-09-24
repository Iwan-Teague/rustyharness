//! Per-attempt layout under a run directory (design §2.8):
//! `<state_root>/runs/<run-id>/attempt-<n>/{journal.jsonl, blobs/}`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The journal file in an attempt directory.
pub const JOURNAL_FILE: &str = "journal.jsonl";
/// The blob store in an attempt directory.
pub const BLOBS_DIR: &str = "blobs";

/// `attempt-<n>` under `run_dir`.
pub fn attempt_dir(run_dir: &Path, n: u32) -> PathBuf {
    run_dir.join(format!("attempt-{n}"))
}

/// Parse `attempt-<n>` (decimal, no sign, no leading zero, n ≥ 1).
pub fn parse_attempt_name(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("attempt-")?;
    let ok = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if ok {
        digits.parse().ok()
    } else {
        None
    }
}

/// Create the next attempt directory: one past the highest existing
/// `attempt-<n>` (1 if none). `create_dir`, not `create_dir_all`: if another
/// process created it first, this fails instead of sharing it.
pub fn create_next_attempt(run_dir: &Path) -> io::Result<(u32, PathBuf)> {
    let mut max = 0u32;
    for entry in fs::read_dir(run_dir)? {
        let name = entry?.file_name();
        if let Some(n) = name.to_str().and_then(parse_attempt_name) {
            max = max.max(n);
        }
    }
    let n = max
        .checked_add(1)
        .ok_or_else(|| io::Error::other("attempt counter exhausted"))?;
    let dir = attempt_dir(run_dir, n);
    fs::create_dir(&dir)?;
    Ok((n, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_names_are_strict() {
        assert_eq!(parse_attempt_name("attempt-1"), Some(1));
        assert_eq!(parse_attempt_name("attempt-42"), Some(42));
        for bad in [
            "attempt-0",
            "attempt-01",
            "attempt-",
            "attempt--1",
            "attempt-1a",
            "x-1",
        ] {
            assert_eq!(parse_attempt_name(bad), None, "{bad}");
        }
    }
}
