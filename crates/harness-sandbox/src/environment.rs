//! The per-OS environment probe (design §7.1, R3 H-17). It MEASURES; the
//! vocabulary, the closed sets of methods and reasons, and the pressure
//! rule are `harness_core::environment`. Nothing here decides an outcome.
//!
//! - **CPUs, every OS:** `std::thread::available_parallelism`.
//! - **Linux:** `/proc/loadavg` (the first field) and `/proc/meminfo`
//!   (`MemTotal`, `MemAvailable`), each read bounded and parsed strictly.
//! - **macOS:** `/usr/sbin/sysctl -n vm.loadavg hw.memsize` and
//!   `/usr/bin/vm_stat`, run by absolute path with fixed arguments, stdin
//!   null, output bounded (the same pattern as the locality probe's
//!   `/sbin/mount`; `statfs`/`sysctl(3)` would need FFI, §6.7).
//!   Memory available is (pages free + pages inactive) times the page size
//!   `vm_stat` reports: macOS has no single "available" figure, so the
//!   method names the formula.
//! - **Windows:** load average does not exist (`no_such_measure`); memory
//!   needs `GlobalMemoryStatusEx`, i.e. `harness-sandbox-windows` (spike
//!   S-W1), so it is `no_safe_api` until then.
//! - **Free bytes on the `state_root` volume, every OS:** `no_safe_api`
//!   (`statvfs(2)` / `GetDiskFreeSpaceExW` need FFI, §6.7).
//!
//! Anything that cannot be read or does not parse exactly is
//! `read_failed`, never a guessed or zero value.

use harness_core::environment::{EnvProbe, EnvSample, Method, Reading, Unmeasured};

/// Cap on what a probe source may return (the files and command outputs
/// here are a few KiB).
pub const SOURCE_MAX_BYTES: u64 = 64 * 1024;

/// The real per-OS probe. The binary gives it to the run driver.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl EnvProbe for SystemEnv {
    fn sample(&self) -> EnvSample {
        let mut s = os::sample();
        s.cpus = match std::thread::available_parallelism()
            .ok()
            .and_then(|n| u64::try_from(n.get()).ok())
        {
            Some(n) => measured(n, Method::AvailableParallelism),
            None => Reading::Unmeasured(Unmeasured::ReadFailed),
        };
        s.state_root_free_bytes = Reading::Unmeasured(Unmeasured::NoSafeApi);
        s
    }
}

fn measured(value: u64, method: Method) -> Reading {
    Reading::Measured { value, method }
}

fn or_failed(v: Option<u64>, method: Method) -> Reading {
    v.map_or(Reading::Unmeasured(Unmeasured::ReadFailed), |value| {
        measured(value, method)
    })
}

/// A decimal like `1.52` in thousandths (`1520`): digits, then optionally
/// `.` and one to three digits. Anything else is `None`.
fn decimal_milli(s: &str) -> Option<u64> {
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) || (s.contains('.') && !digits(frac)) || frac.len() > 3 {
        return None;
    }
    let mut milli: u64 = 0;
    for (i, b) in frac
        .bytes()
        .chain(std::iter::repeat(b'0'))
        .take(3)
        .enumerate()
    {
        let place = [100u64, 10, 1].get(i).copied()?;
        milli = milli.checked_add(u64::from(b - b'0') * place)?;
    }
    int.parse::<u64>()
        .ok()?
        .checked_mul(1000)?
        .checked_add(milli)
}

/// Linux `/proc/loadavg`: the 1-minute load in thousandths.
pub fn parse_loadavg(text: &str) -> Option<u64> {
    decimal_milli(text.split_ascii_whitespace().next()?)
}

/// Linux `/proc/meminfo`: `(MemTotal, MemAvailable)` in bytes. Each key
/// must appear at most once with a `kB` value; a repeated key makes that
/// field unreadable rather than picking one.
pub fn parse_meminfo(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |key: &str| -> Option<u64> {
        let mut found = None;
        for line in text.lines() {
            let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            let mut parts = rest.split_ascii_whitespace();
            let (Some(n), Some("kB"), None) = (parts.next(), parts.next(), parts.next()) else {
                return None;
            };
            if !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            found = Some(n.parse::<u64>().ok()?.checked_mul(1024)?);
        }
        found
    };
    (field("MemTotal"), field("MemAvailable"))
}

/// macOS `sysctl -n vm.loadavg hw.memsize`: `(load in thousandths,
/// memsize in bytes)`. The output is exactly two lines: `{ 1.52 1.70 1.78 }`
/// and a byte count.
pub fn parse_sysctl(text: &str) -> (Option<u64>, Option<u64>) {
    let mut lines = text.lines();
    let (Some(load), Some(mem), None) = (lines.next(), lines.next(), lines.next()) else {
        return (None, None);
    };
    let load = load
        .strip_prefix("{ ")
        .and_then(|l| l.strip_suffix(" }"))
        .and_then(|l| l.split(' ').next())
        .and_then(decimal_milli);
    let mem = if !mem.is_empty() && mem.bytes().all(|b| b.is_ascii_digit()) {
        mem.parse::<u64>().ok()
    } else {
        None
    };
    (load, mem)
}

/// macOS `vm_stat`: (pages free + pages inactive) times the page size its
/// first line states. Each counter must appear exactly once.
pub fn parse_vm_stat(text: &str) -> Option<u64> {
    let mut lines = text.lines();
    let page = lines
        .next()?
        .strip_prefix("Mach Virtual Memory Statistics: (page size of ")?
        .strip_suffix(" bytes)")?;
    if page.is_empty() || !page.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let page: u64 = page.parse().ok()?;
    let count = |key: &str| -> Option<u64> {
        let mut found = None;
        for line in text.lines() {
            let Some(rest) = line.strip_prefix(key) else {
                continue;
            };
            if found.is_some() {
                return None;
            }
            let n = rest.trim_start().strip_suffix('.')?;
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            found = Some(n.parse::<u64>().ok()?);
        }
        found
    };
    count("Pages free:")?
        .checked_add(count("Pages inactive:")?)?
        .checked_mul(page)
}

#[cfg(target_os = "linux")]
mod os {
    use std::io::Read;

    use harness_core::environment::{EnvSample, Method, Unmeasured};

    use super::{or_failed, parse_loadavg, parse_meminfo, SOURCE_MAX_BYTES};

    fn read_small(path: &str) -> Option<String> {
        let mut text = String::new();
        std::fs::File::open(path)
            .ok()?
            .take(SOURCE_MAX_BYTES + 1)
            .read_to_string(&mut text)
            .ok()?;
        (text.len() as u64 <= SOURCE_MAX_BYTES).then_some(text)
    }

    pub(super) fn sample() -> EnvSample {
        let mut s = EnvSample::unmeasured(Unmeasured::ReadFailed);
        s.load_1m_milli = or_failed(
            read_small("/proc/loadavg").and_then(|t| parse_loadavg(&t)),
            Method::ProcLoadavg,
        );
        let (total, avail) = read_small("/proc/meminfo")
            .map(|t| parse_meminfo(&t))
            .unwrap_or((None, None));
        s.mem_total_bytes = or_failed(total, Method::ProcMeminfoTotal);
        s.mem_available_bytes = or_failed(avail, Method::ProcMeminfoAvailable);
        s
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::process::{Command, Output, Stdio};

    use harness_core::environment::{EnvSample, Method, Unmeasured};

    use super::{or_failed, parse_sysctl, parse_vm_stat, SOURCE_MAX_BYTES};

    fn text(out: std::io::Result<Output>) -> Option<String> {
        let out = out.ok().filter(|o| o.status.success())?;
        if out.stdout.len() as u64 > SOURCE_MAX_BYTES {
            return None;
        }
        String::from_utf8(out.stdout).ok()
    }

    pub(super) fn sample() -> EnvSample {
        let mut s = EnvSample::unmeasured(Unmeasured::ReadFailed);
        let (load, mem) = text(
            Command::new("/usr/sbin/sysctl")
                .args(["-n", "vm.loadavg", "hw.memsize"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output(),
        )
        .map(|t| parse_sysctl(&t))
        .unwrap_or((None, None));
        s.load_1m_milli = or_failed(load, Method::SysctlVmLoadavg);
        s.mem_total_bytes = or_failed(mem, Method::SysctlHwMemsize);
        s.mem_available_bytes = or_failed(
            text(
                Command::new("/usr/bin/vm_stat")
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .output(),
            )
            .and_then(|t| parse_vm_stat(&t)),
            Method::VmStatFreeInactive,
        );
        s
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
    use harness_core::environment::{EnvSample, Reading, Unmeasured};

    pub(super) fn sample() -> EnvSample {
        let mut s = EnvSample::unmeasured(Unmeasured::NoSafeApi);
        if cfg!(windows) {
            s.load_1m_milli = Reading::Unmeasured(Unmeasured::NoSuchMeasure);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimals_are_exact_thousandths_or_refused() {
        assert_eq!(decimal_milli("1.52"), Some(1520));
        assert_eq!(decimal_milli("0.05"), Some(50));
        assert_eq!(decimal_milli("12"), Some(12_000));
        assert_eq!(decimal_milli("3.141"), Some(3141));
        for bad in [
            "",
            ".5",
            "1.",
            "1.2345",
            "-1.0",
            "1,5",
            "1.5x",
            " 1.5",
            "18446744073709552",
        ] {
            assert_eq!(decimal_milli(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn linux_sources_parse_strictly() {
        assert_eq!(parse_loadavg("0.52 0.58 0.59 1/467 12345\n"), Some(520));
        assert_eq!(parse_loadavg(""), None);
        let mi = "MemTotal:       16318756 kB\nMemFree:          812344 kB\nMemAvailable:    9876543 kB\n";
        assert_eq!(
            parse_meminfo(mi),
            (Some(16_318_756 * 1024), Some(9_876_543 * 1024))
        );
        // A repeated key is unreadable, never "the last one wins".
        let dup = format!("{mi}MemAvailable:    1 kB\n");
        assert_eq!(parse_meminfo(&dup), (Some(16_318_756 * 1024), None));
        // Wrong unit, trailing junk, missing field.
        assert_eq!(parse_meminfo("MemTotal: 5 MB\n"), (None, None));
        assert_eq!(parse_meminfo("MemTotal: 5 kB x\n"), (None, None));
        assert_eq!(parse_meminfo("MemTotal: 5 kB\n"), (Some(5 * 1024), None));
        // "MemTotalX:" is not "MemTotal:".
        assert_eq!(parse_meminfo("MemTotalX: 5 kB\n"), (None, None));
    }

    #[test]
    fn macos_sources_parse_strictly() {
        assert_eq!(
            parse_sysctl("{ 1.52 1.70 1.78 }\n17179869184\n"),
            (Some(1520), Some(17_179_869_184))
        );
        assert_eq!(parse_sysctl("{ 1.52 1.70 1.78 }\n"), (None, None));
        assert_eq!(
            parse_sysctl("1.52 1.70 1.78\n17179869184\n"),
            (None, Some(17_179_869_184))
        );
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                  Pages free:                               12345.\n\
                  Pages active:                            200000.\n\
                  Pages inactive:                          100000.\n\
                  Pages speculative:                         1234.\n";
        assert_eq!(parse_vm_stat(vm), Some((12_345 + 100_000) * 16_384));
        assert_eq!(
            parse_vm_stat(&format!("{vm}Pages free: 1.\n")),
            None,
            "a repeated counter is refused"
        );
        assert_eq!(
            parse_vm_stat(&vm.replace("page size of 16384", "page size of x")),
            None
        );
        assert_eq!(parse_vm_stat(&vm.replace("100000.", "100000")), None);
    }

    #[test]
    fn this_host_is_sampled_with_named_methods_and_never_a_fake_zero() {
        let s = SystemEnv.sample();
        assert!(
            matches!(
                s.cpus,
                Reading::Measured { value, method: Method::AvailableParallelism } if value > 0
            ),
            "{s:?}"
        );
        assert_eq!(
            s.state_root_free_bytes,
            Reading::Unmeasured(Unmeasured::NoSafeApi)
        );
        if cfg!(any(target_os = "linux", target_os = "macos")) {
            for r in [s.load_1m_milli, s.mem_total_bytes, s.mem_available_bytes] {
                assert!(matches!(r, Reading::Measured { .. }), "{s:?}");
            }
            if let (
                Reading::Measured { value: total, .. },
                Reading::Measured { value: avail, .. },
            ) = (s.mem_total_bytes, s.mem_available_bytes)
            {
                assert!(total > 0 && avail <= total, "{s:?}");
            }
        } else if cfg!(windows) {
            assert_eq!(
                s.load_1m_milli,
                Reading::Unmeasured(Unmeasured::NoSuchMeasure)
            );
            assert_eq!(
                s.mem_total_bytes,
                Reading::Unmeasured(Unmeasured::NoSafeApi)
            );
        }
    }
}
