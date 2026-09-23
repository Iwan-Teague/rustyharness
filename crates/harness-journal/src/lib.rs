//! Run journal: append-only evidence of what a run did.
//!
//! SCAFFOLD (2026-09-23). Settled direction: a run's result is extracted from
//! evidence (gate outcomes, diffs, test output) recorded here, never from the
//! agent's own claim of success. Record format should align with the suite's
//! gate-run records design (`charter/design/gate-run-records-v0.1.md`) and
//! rustybenchmark's JSONL journal; to be decided in the rustyharness design.

#![forbid(unsafe_code)]

/// Something that happened during a run, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The run started with this task text.
    Started {
        /// The task as given.
        task: String,
    },
    /// A capability was invoked.
    Invoked {
        /// Capability id, e.g. `rustydns.zone.read`.
        capability: String,
    },
    /// A policy or confinement refusal.
    Refused {
        /// Why.
        reason: String,
    },
}

/// Where events go. Implementations must be append-only.
pub trait Sink {
    /// Append one event.
    fn append(&mut self, event: Event) -> std::io::Result<()>;
}
