//! The environment sample (design §7.1, R3 H-17): the host's condition,
//! recorded as data, so that a timeout or a crash under host pressure can be
//! told apart from a broken tool without re-running anything.
//!
//! This module is the vocabulary only. The measuring probe does I/O and
//! lives in `harness-sandbox` (`environment::SystemEnv`); the run driver is
//! given it, as it is given the locality probe. The rules it encodes:
//!
//! - A field the platform cannot supply through a safe API is
//!   [`Reading::Unmeasured`] with its reason, never zero.
//! - Every method and every reason is harness text from a closed set
//!   ([`Method`], [`Unmeasured`]), so no measured value carries text from
//!   outside, and a recorded sample parses back exactly (audit replay
//!   re-feeds recorded samples; it cannot re-measure a past host).
//! - The sample never changes an outcome.
//!   [`EnvSample::possibly_environmental`] only lets a report add an Info
//!   finding.

/// Where a sample comes from: the run driver's seam. The binary's probe
/// measures the host; tests and audit replay give a fixed [`EnvSample`].
pub trait EnvProbe {
    /// Sample the host now.
    fn sample(&self) -> EnvSample;
}

/// A fixed sample is its own probe.
impl EnvProbe for EnvSample {
    fn sample(&self) -> EnvSample {
        *self
    }
}

/// One environment sample (design §7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvSample {
    /// Logical CPUs available to this process.
    pub cpus: Reading,
    /// The 1-minute load average, in thousandths (1.5 is 1500).
    pub load_1m_milli: Reading,
    /// Memory total, in bytes.
    pub mem_total_bytes: Reading,
    /// Memory available, in bytes.
    pub mem_available_bytes: Reading,
    /// Free bytes on the volume holding `state_root`.
    pub state_root_free_bytes: Reading,
}

impl EnvSample {
    /// The fields in their journal order, with their journal keys.
    pub fn fields(&self) -> [(&'static str, Reading); 5] {
        [
            ("cpus", self.cpus),
            ("load_1m_milli", self.load_1m_milli),
            ("mem_total_bytes", self.mem_total_bytes),
            ("mem_available_bytes", self.mem_available_bytes),
            ("state_root_free_bytes", self.state_root_free_bytes),
        ]
    }

    /// A sample with every field unmeasured for `why`.
    pub const fn unmeasured(why: Unmeasured) -> Self {
        let u = Reading::Unmeasured(why);
        Self {
            cpus: u,
            load_1m_milli: u,
            mem_total_bytes: u,
            mem_available_bytes: u,
            state_root_free_bytes: u,
        }
    }

    /// Design §7.1: memory available under 5% of the total, or a 1-minute
    /// load above twice the CPU count. Each half needs both of its fields
    /// measured; an unmeasured field never makes the host look pressed.
    pub fn possibly_environmental(&self) -> bool {
        let low_memory = match (self.mem_available_bytes, self.mem_total_bytes) {
            (Reading::Measured { value: avail, .. }, Reading::Measured { value: total, .. }) => {
                total > 0 && u128::from(avail) * 20 < u128::from(total)
            }
            _ => false,
        };
        let overloaded = match (self.load_1m_milli, self.cpus) {
            (Reading::Measured { value: load, .. }, Reading::Measured { value: cpus, .. }) => {
                u128::from(load) > u128::from(cpus) * 2 * 1000
            }
            _ => false,
        };
        low_memory || overloaded
    }
}

/// One field of a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// Measured, with the method that produced it.
    Measured {
        /// The value, in the field's unit.
        value: u64,
        /// How it was measured.
        method: Method,
    },
    /// Not measured, and why.
    Unmeasured(Unmeasured),
}

/// How a field was measured (the closed set of producing methods).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// The standard library's `available_parallelism` (CPU affinity and
    /// cgroup quotas included).
    AvailableParallelism,
    /// Linux `/proc/loadavg`, the first field.
    ProcLoadavg,
    /// Linux `/proc/meminfo`, `MemTotal`.
    ProcMeminfoTotal,
    /// Linux `/proc/meminfo`, `MemAvailable`.
    ProcMeminfoAvailable,
    /// macOS `/usr/sbin/sysctl -n vm.loadavg`, the first value.
    SysctlVmLoadavg,
    /// macOS `/usr/sbin/sysctl -n hw.memsize`.
    SysctlHwMemsize,
    /// macOS `/usr/bin/vm_stat`: (pages free + pages inactive) times the
    /// page size it reports.
    VmStatFreeInactive,
}

impl Method {
    /// Every method.
    pub const ALL: [Method; 7] = [
        Method::AvailableParallelism,
        Method::ProcLoadavg,
        Method::ProcMeminfoTotal,
        Method::ProcMeminfoAvailable,
        Method::SysctlVmLoadavg,
        Method::SysctlHwMemsize,
        Method::VmStatFreeInactive,
    ];

    /// The journal name.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::AvailableParallelism => "available_parallelism",
            Method::ProcLoadavg => "/proc/loadavg",
            Method::ProcMeminfoTotal => "/proc/meminfo MemTotal",
            Method::ProcMeminfoAvailable => "/proc/meminfo MemAvailable",
            Method::SysctlVmLoadavg => "sysctl vm.loadavg",
            Method::SysctlHwMemsize => "sysctl hw.memsize",
            Method::VmStatFreeInactive => "vm_stat free+inactive",
        }
    }

    /// Parse a journal name; anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

/// Why a field was not measured (the closed set of reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasured {
    /// This platform supplies it only through an API this build cannot
    /// call safely (a system call that needs FFI; design §6.7). Free disk
    /// space everywhere, and memory on Windows until spike S-W1.
    NoSafeApi,
    /// The platform has no such measure (Windows has no load average).
    NoSuchMeasure,
    /// The source exists but could not be read or understood.
    ReadFailed,
}

impl Unmeasured {
    /// Every reason.
    pub const ALL: [Unmeasured; 3] = [
        Unmeasured::NoSafeApi,
        Unmeasured::NoSuchMeasure,
        Unmeasured::ReadFailed,
    ];

    /// The journal name.
    pub fn as_str(self) -> &'static str {
        match self {
            Unmeasured::NoSafeApi => "no_safe_api",
            Unmeasured::NoSuchMeasure => "no_such_measure",
            Unmeasured::ReadFailed => "read_failed",
        }
    }

    /// Parse a journal name; anything else is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|u| u.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(value: u64) -> Reading {
        Reading::Measured {
            value,
            method: Method::AvailableParallelism,
        }
    }

    fn sample(cpus: Reading, load: Reading, total: Reading, avail: Reading) -> EnvSample {
        EnvSample {
            cpus,
            load_1m_milli: load,
            mem_total_bytes: total,
            mem_available_bytes: avail,
            state_root_free_bytes: Reading::Unmeasured(Unmeasured::NoSafeApi),
        }
    }

    #[test]
    fn names_round_trip_and_are_distinct() {
        for m in Method::ALL {
            assert_eq!(Method::parse(m.as_str()), Some(m));
        }
        for u in Unmeasured::ALL {
            assert_eq!(Unmeasured::parse(u.as_str()), Some(u));
        }
        assert_eq!(Method::parse(""), None);
        assert_eq!(Method::parse("/proc/loadavg "), None);
        assert_eq!(Unmeasured::parse("NoSafeApi"), None);
    }

    #[test]
    fn pressure_rule_is_five_percent_memory_or_twice_the_cpus() {
        let u = Reading::Unmeasured(Unmeasured::ReadFailed);
        // Memory: 4.9% available is pressure, 5% is not.
        assert!(sample(u, u, m(1000), m(49)).possibly_environmental());
        assert!(!sample(u, u, m(1000), m(50)).possibly_environmental());
        // Load: above 2x the CPUs is pressure, exactly 2x is not.
        assert!(sample(m(4), m(8001), u, u).possibly_environmental());
        assert!(!sample(m(4), m(8000), u, u).possibly_environmental());
        // No overflow at the extremes.
        assert!(
            !sample(m(u64::MAX), m(u64::MAX), m(u64::MAX), m(u64::MAX)).possibly_environmental()
        );
    }

    #[test]
    fn unmeasured_fields_never_look_like_pressure() {
        let u = Reading::Unmeasured(Unmeasured::NoSafeApi);
        // Available memory unmeasured next to a real total: no claim.
        assert!(!sample(u, u, m(1000), u).possibly_environmental());
        // A load with no CPU count: no claim.
        assert!(!sample(u, m(u64::MAX), u, u).possibly_environmental());
        assert!(!EnvSample::unmeasured(Unmeasured::NoSafeApi).possibly_environmental());
        // A zero total is not "0% available".
        assert!(!sample(u, u, m(0), m(0)).possibly_environmental());
    }
}
