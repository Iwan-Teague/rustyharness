//! Capability manifests: the seam that lets suite apps slot into the harness
//! without the harness being redesigned.
//!
//! SCAFFOLD (2026-09-23). The idea being designed (docs/00-overview.md §4):
//! each app ships a versioned manifest declaring its capabilities — what each
//! one does, what EFFECT class it has, what confinement it needs. The harness
//! core knows effect classes and policy, never app specifics, so a new app is
//! a new manifest plus an adapter, not a harness change. Wire protocol (native
//! vs MCP-compatible vs both) is an open design question.
//!
//! What IS settled here: validation is a content check, and anything the
//! harness does not understand is refused, never ignored.

#![forbid(unsafe_code)]
// The panic-set lints ratchet production code; unit tests may assert loosely.
#![cfg_attr(
    test,
    allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use serde::Deserialize;

/// The manifest schema version this harness understands.
pub const SCHEMA_VERSION: u32 = 0;

/// Length cap for `app` names, in bytes.
pub const APP_MAX: usize = 64;
/// Length cap for capability ids, in bytes.
pub const ID_MAX: usize = 128;
/// Length cap for capability summaries, in bytes.
pub const SUMMARY_MAX: usize = 512;
/// Length cap for `app_version`, in bytes.
pub const APP_VERSION_MAX: usize = 32;
/// Cap on how many capabilities one manifest may declare.
pub const CAPABILITIES_MAX: usize = 1024;

/// An app's declaration of the capabilities it offers to agents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest schema version; must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The app, e.g. `rustydns`.
    pub app: String,
    /// The app's own version.
    pub app_version: String,
    /// What the app offers.
    pub capabilities: Vec<Capability>,
}

/// One thing an agent may ask an app to do.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// Stable id, `<app>.<verb>`, e.g. `rustydns.zone.read`.
    pub id: String,
    /// Human-readable purpose, shown in approval prompts.
    pub summary: String,
    /// What class of effect it has; drives policy and approval.
    pub effect: Effect,
}

/// Effect classes, ordered by how much harm a misuse can do. Policy is written
/// against these, not against app-specific capability ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Reads non-secret state.
    Read,
    /// Changes state inside the app, reversibly.
    Write,
    /// Runs code or commands.
    Execute,
    /// Changes state irreversibly or outside this machine (delete, publish, send).
    Irreversible,
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Schema version this harness does not understand.
    #[error("unsupported manifest schema_version {0} (this harness understands {SCHEMA_VERSION})")]
    UnsupportedSchema(u32),
    /// A manifest that declares nothing is refused, not treated as harmless.
    #[error("manifest for {0} declares no capabilities")]
    Empty(String),
    /// A capability id not namespaced under its own app.
    #[error("capability {id} is not namespaced under app {app}")]
    ForeignNamespace {
        /// The declaring app.
        app: String,
        /// The offending capability id.
        id: String,
    },
    /// Two capabilities share an id.
    #[error("duplicate capability id {0}")]
    Duplicate(String),
    /// A name (app or capability id) is outside the manifest name grammar.
    #[error("{field} {value:?} is not a valid name: use 1..={max} bytes of ASCII lowercase letters, digits, '-' or '_'")]
    InvalidName {
        /// Which field: `"app"` or `"capability id"`.
        field: &'static str,
        /// The offending value.
        value: String,
        /// The length cap that also applies to this name.
        max: usize,
    },
    /// A field exceeds its length cap.
    #[error("{field} is {len} bytes; the cap is {max}")]
    NameTooLong {
        /// Which field.
        field: &'static str,
        /// Actual length in bytes.
        len: usize,
        /// The cap.
        max: usize,
    },
}

/// A byte allowed in a manifest name. Dots are the namespace separator, so
/// they are legal only inside capability ids, never in the `app` itself.
fn is_name_byte(b: u8, allow_dot: bool) -> bool {
    b.is_ascii_lowercase()
        || b.is_ascii_digit()
        || b == b'-'
        || b == b'_'
        || (allow_dot && b == b'.')
}

/// Check one name against the grammar and its length cap. Empty names and
/// non-ASCII bytes are refused: ids are matched against policy written by
/// humans, so anything lookalike or invisible is a footgun, not a name.
fn check_name(
    field: &'static str,
    value: &str,
    allow_dot: bool,
    max: usize,
) -> Result<(), ManifestError> {
    if value.is_empty() || !value.bytes().all(|b| is_name_byte(b, allow_dot)) {
        return Err(ManifestError::InvalidName {
            field,
            value: value.to_string(),
            max,
        });
    }
    if value.len() > max {
        return Err(ManifestError::NameTooLong {
            field,
            len: value.len(),
            max,
        });
    }
    Ok(())
}

impl Manifest {
    /// Check the manifest's CONTENT, not just that it parsed.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }
        check_name("app", &self.app, false, APP_MAX)?;
        if self.app_version.len() > APP_VERSION_MAX {
            return Err(ManifestError::NameTooLong {
                field: "app_version",
                len: self.app_version.len(),
                max: APP_VERSION_MAX,
            });
        }
        if self.capabilities.is_empty() {
            return Err(ManifestError::Empty(self.app.clone()));
        }
        if self.capabilities.len() > CAPABILITIES_MAX {
            return Err(ManifestError::NameTooLong {
                field: "capabilities",
                len: self.capabilities.len(),
                max: CAPABILITIES_MAX,
            });
        }
        let prefix = format!("{}.", self.app);
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.capabilities {
            check_name("capability id", &c.id, true, ID_MAX)?;
            let Some(rest) = c.id.strip_prefix(&prefix) else {
                return Err(ManifestError::ForeignNamespace {
                    app: self.app.clone(),
                    id: c.id.clone(),
                });
            };
            if rest.is_empty()
                || rest.starts_with('.')
                || rest.ends_with('.')
                || rest.contains("..")
            {
                return Err(ManifestError::InvalidName {
                    field: "capability id",
                    value: c.id.clone(),
                    max: ID_MAX,
                });
            }
            if c.summary.len() > SUMMARY_MAX {
                return Err(ManifestError::NameTooLong {
                    field: "summary",
                    len: c.summary.len(),
                    max: SUMMARY_MAX,
                });
            }
            if !seen.insert(c.id.as_str()) {
                return Err(ManifestError::Duplicate(c.id.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Manifest {
        serde_json::from_str(s).expect("test manifest parses")
    }

    const GOOD: &str = r#"{"schema_version":0,"app":"rustydns","app_version":"0.1.0",
        "capabilities":[{"id":"rustydns.zone.read","summary":"Read a zone","effect":"read"}]}"#;

    #[test]
    fn good_manifest_validates() {
        assert_eq!(parse(GOOD).validate(), Ok(()));
    }

    #[test]
    fn unknown_fields_are_refused_not_ignored() {
        let s = GOOD.replace("\"app_version\"", "\"surprise\":1,\"app_version\"");
        assert!(serde_json::from_str::<Manifest>(&s).is_err());
    }

    #[test]
    fn empty_foreign_and_duplicate_are_refused() {
        let mut m = parse(GOOD);
        m.capabilities.clear();
        assert!(matches!(m.validate(), Err(ManifestError::Empty(_))));

        let mut m = parse(GOOD);
        m.capabilities[0].id = "rustyvault.secret.read".into();
        assert!(matches!(
            m.validate(),
            Err(ManifestError::ForeignNamespace { .. })
        ));

        let mut m = parse(GOOD);
        m.capabilities.push(m.capabilities[0].clone());
        assert!(matches!(m.validate(), Err(ManifestError::Duplicate(_))));
    }

    #[test]
    fn future_schema_is_refused() {
        let mut m = parse(GOOD);
        m.schema_version = 1;
        assert_eq!(m.validate(), Err(ManifestError::UnsupportedSchema(1)));
    }

    // Review attacks (2026-09-23): a content check must refuse these. Each
    // asserts only refusal; the error variant is the validator's business.
    #[test]
    fn empty_app_is_refused() {
        let mut m = parse(GOOD);
        m.app = String::new();
        m.capabilities[0].id = ".zone.read".into();
        assert!(m.validate().is_err(), "empty app must not validate");
    }

    #[test]
    fn ids_with_whitespace_or_unicode_are_refused() {
        for bad in [
            " rustydns.zone.read",         // control: refused even today
            "rustydns.zone.read ",         // trailing space
            "rustydns .zone.read",         // internal space
            "rustydns.zone.re\tad",        // tab
            "rustydns.zone.re\nad",        // newline
            "rustydns.zone.re\u{FF0E}ad",  // fullwidth dot
            "rustydns.zone.re\u{1F600}ad", // emoji
        ] {
            let mut m = parse(GOOD);
            m.capabilities[0].id = bad.to_string();
            assert!(m.validate().is_err(), "id {bad:?} must not validate");
        }
    }

    #[test]
    fn uppercase_in_id_is_refused() {
        let mut m = parse(GOOD);
        m.capabilities[0].id = "rustydns.Zone.read".into();
        assert!(m.validate().is_err(), "uppercase id must not validate");
    }

    #[test]
    fn oversized_names_are_refused() {
        let mut m = parse(GOOD);
        m.capabilities[0].id = format!("rustydns.zone.{}", "a".repeat(10_000));
        assert!(m.validate().is_err(), "oversized id must not validate");

        let mut m = parse(GOOD);
        m.app = "a".repeat(10_000);
        assert!(m.validate().is_err(), "oversized app must not validate");
    }

    #[test]
    fn case_differing_duplicate_ids_are_refused() {
        let mut m = parse(GOOD);
        let upper: String = "rustydns.zone.read"
            .chars()
            .flat_map(|c| c.to_uppercase())
            .collect();
        m.capabilities[0].id = upper.clone();
        m.capabilities.push(Capability {
            id: "rustydns.zone.read".into(),
            summary: "Read a zone".into(),
            effect: Effect::Read,
        });
        assert!(
            m.validate().is_err(),
            "case-differing duplicate ids must not validate"
        );
        let _ = upper;
    }
}
