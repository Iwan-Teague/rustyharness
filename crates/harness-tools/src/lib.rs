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

use serde::Deserialize;

/// The manifest schema version this harness understands.
pub const SCHEMA_VERSION: u32 = 0;

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
}

impl Manifest {
    /// Check the manifest's CONTENT, not just that it parsed.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ManifestError::UnsupportedSchema(self.schema_version));
        }
        if self.capabilities.is_empty() {
            return Err(ManifestError::Empty(self.app.clone()));
        }
        let prefix = format!("{}.", self.app);
        let mut seen = std::collections::BTreeSet::new();
        for c in &self.capabilities {
            if !c.id.starts_with(&prefix) || c.id.len() == prefix.len() {
                return Err(ManifestError::ForeignNamespace {
                    app: self.app.clone(),
                    id: c.id.clone(),
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
}
