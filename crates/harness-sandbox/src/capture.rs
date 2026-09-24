//! Bounded capture of a fixed system query's standard output: the macOS
//! probes' `/sbin/mount` (locality, §2.8), `/usr/sbin/sysctl` and
//! `/usr/bin/vm_stat` (environment sample, §7.1).
//!
//! These are the harness's only unconfined children besides those §4.5
//! names, and each is read-only, run by absolute path with a fixed argv
//! (no payload reaches it, INV-23). The capture bounds everything the child
//! could do to the harness (H1f-3 review F-7): its environment is cleared
//! and `LC_ALL=C` set (a locale cannot change the number format), stdin is
//! null, stderr is discarded, at most [`CAPTURE_MAX_BYTES`] + 1 bytes of
//! stdout are read, and past the deadline the child is killed. Anything but
//! a clean exit within the bounds is an error, which the probes turn into a
//! refusal or `read_failed`.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Cap on a captured stdout: the locality probe's mount-table cap (a
/// query's output is a few KiB; the cap only bounds a misbehaving child).
pub const CAPTURE_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// How long a system query may take.
pub const CAPTURE_DEADLINE: Duration = Duration::from_secs(5);

/// Run `cmd` (its program and argv already fixed by the caller) and return
/// its stdout, within the bounds above.
pub(crate) fn capture(mut cmd: Command, deadline: Duration) -> Result<Vec<u8>, String> {
    let until = Instant::now() + deadline;
    let mut child = cmd
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run: {e}"))?;
    let Some(mut out) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("no stdout pipe".into());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let r = (&mut out)
            .take(CAPTURE_MAX_BYTES + 1)
            .read_to_end(&mut buf)
            .map(|_| buf);
        let _ = tx.send(r);
    });
    let read = rx.recv_timeout(until.saturating_duration_since(Instant::now()));
    let buf = match read {
        Ok(Ok(buf)) if buf.len() as u64 <= CAPTURE_MAX_BYTES => buf,
        Ok(Ok(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("output over the cap".into());
        }
        Ok(Err(e)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("output unreadable: {e}"));
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("no complete output before the deadline".into());
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(buf),
            Ok(Some(status)) => return Err(format!("exited with {status}")),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("did not exit before the deadline".into());
            }
            Err(e) => return Err(format!("cannot wait: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each command is built inline from literals: this file is under
    // crates/*/src, where purity.sh §2f (INV-23) allows only fixed spawns.

    #[test]
    fn a_clean_exit_returns_its_output_with_a_c_locale_and_no_inherited_env() {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "echo \"$LC_ALL:${HOME:-unset}\""]);
        assert_eq!(capture(c, CAPTURE_DEADLINE).unwrap(), b"C:unset\n");
    }

    #[test]
    fn failure_oversize_and_a_hang_are_errors() {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "exit 3"]);
        assert!(capture(c, CAPTURE_DEADLINE).unwrap_err().contains("exited"));

        let mut c = Command::new("/bin/sh");
        c.args(["-c", "head -c 5000000 /dev/zero"]);
        let big = capture(c, CAPTURE_DEADLINE).unwrap_err();
        assert!(big.contains("over the cap"), "{big}");

        let t = Instant::now();
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "sleep 30"]);
        let hung = capture(c, Duration::from_millis(200)).unwrap_err();
        assert!(hung.contains("deadline"), "{hung}");
        assert!(t.elapsed() < Duration::from_secs(10));

        // Output complete, but the child lingers: still an error.
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "echo x; exec 1>&-; sleep 30"]);
        let lingering = capture(c, Duration::from_millis(300)).unwrap_err();
        assert!(lingering.contains("did not exit"), "{lingering}");
    }
}
