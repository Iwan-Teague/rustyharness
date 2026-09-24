//! Per-attempt layout under a run directory (design §2.8):
//! `<state_root>/runs/<run-id>/attempt-<n>/{journal.jsonl, blobs/}`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use harness_core::RunId;

/// The journal file in an attempt directory.
pub const JOURNAL_FILE: &str = "journal.jsonl";
/// The blob store in an attempt directory.
pub const BLOBS_DIR: &str = "blobs";

/// `state_root/runs/<run-id>` (design §2.8). The id is a [`RunId`]: 32
/// lowercase hex characters, never `.`, `..` or a path (NF-3).
pub fn run_dir(state_root: &Path, run: &RunId) -> PathBuf {
    state_root.join("runs").join(run.as_str())
}

/// Create `state_root/runs/<run-id>` durably: `runs/` is created if absent
/// (and `state_root` is fsynced every time), the run directory is created with
/// `create_dir` (it must not exist), and `runs/` is fsynced so the new entry
/// survives a crash (the H1c F-1 rule, applied one level up). Neither
/// `runs/` nor the run directory may be a symlink.
pub fn create_run_dir(state_root: &Path, run: &RunId) -> io::Result<PathBuf> {
    create_run_dir_with(state_root, run, &crate::writer::RealDirSync)
}

pub(crate) fn create_run_dir_with(
    state_root: &Path,
    run: &RunId,
    ds: &dyn crate::writer::DirSync,
) -> io::Result<PathBuf> {
    let runs = state_root.join("runs");
    match fs::create_dir(&runs) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // Always, not only when `runs/` was just created: if an earlier attempt
    // created it and then failed to sync `state_root`, this call must not
    // skip the sync (H1e-1 review, durability edge).
    ds.sync(state_root)?;
    let m = fs::symlink_metadata(&runs)?;
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runs/ is not a real directory",
        ));
    }
    let dir = runs.join(run.as_str());
    fs::create_dir(&dir)?;
    ds.sync(&runs)?;
    Ok(dir)
}

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
