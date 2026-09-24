//! Per-model profiles (design §3.4). Pure.
//!
//! A profile is a data file (JSON, strict: duplicate keys and unknown fields
//! refused). Content checks: every number in range, names in their
//! grammar, no field that this build cannot honour:
//! - `grammar` other than `none` is refused (constrained decoding through
//!   the OpenAI-compatible API is spike S-P1; safety never depends on it);
//! - `price` is refused: prices exist for hosted endpoints only, and this
//!   build has none (N-7: "a hosted run without a price table refuses to
//!   start" belongs with the `hosted` feature);
//! - `tool_choice_required_ok: true` with the text protocol is refused as
//!   meaningless.
//!
//! An unknown model gets [`Profile::conservative_default`]. A profile runs
//! whether or not `profile check` stamped it; `profile_validated` is
//! recorded in every journal header (§3.4).

use serde::Deserialize;

use harness_core::{sha256, strict_json, Digest};

/// Action protocol (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OpenAI `tools` / `tool_calls`.
    Native,
    /// One `<action>{…}</action>` block in the reply text.
    Text,
}

/// Constrained decoding (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grammar {
    /// None.
    None,
    /// llama.cpp lazy GBNF.
    GbnfLazy,
    /// JSON-Schema-constrained.
    JsonSchema,
}

/// Edit format (§4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditFormat {
    /// Exact search/replace.
    Replace,
    /// Whole-file write.
    Whole,
}

/// Sampling defaults.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sampling {
    /// 0.0..=2.0.
    pub temperature: f64,
    /// (0.0, 1.0].
    pub top_p: f64,
    /// Optional fixed seed.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Completion cap, 1..context_window.
    pub max_tokens: u64,
}

/// The `profile check` stamp: the digest of the smoke-eval report it passed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    /// SHA-256 (hex) of the report.
    pub report_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileWire {
    profile_version: u32,
    id: String,
    model: String,
    context_window: u64,
    fill_ratio: f64,
    protocol: Protocol,
    tool_choice_required_ok: bool,
    grammar: Grammar,
    max_active_tools: u32,
    edit_format: EditFormat,
    recent_turns: u32,
    sampling: Sampling,
    #[serde(default)]
    kv_quant_note: Option<String>,
    #[serde(default)]
    price: Option<serde_json::Value>,
    #[serde(default)]
    validated: Option<Stamp>,
}

/// A validated profile. Private fields; built by [`Profile::parse`] or
/// [`Profile::conservative_default`].
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    id: String,
    model: String,
    context_window: u64,
    fill_ratio: f64,
    protocol: Protocol,
    tool_choice_required_ok: bool,
    max_active_tools: u32,
    edit_format: EditFormat,
    recent_turns: u32,
    sampling: Sampling,
    kv_quant_note: Option<String>,
    validated: Option<Stamp>,
    sha256: Option<Digest>,
}

/// Why a profile was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    /// Not strict JSON, or an unknown/missing field.
    #[error("profile is not a valid profile document: {0}")]
    Shape(String),
    /// A field out of range or outside its grammar.
    #[error("profile field {0} is out of range or malformed")]
    Field(&'static str),
    /// A field this build cannot honour.
    #[error("profile field {field}: {why}")]
    NotInThisBuild {
        /// Field.
        field: &'static str,
        /// Why.
        why: &'static str,
    },
}

/// Profile schema versions understood.
pub const PROFILE_VERSIONS: &[u32] = &[1];

fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn is_model_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic())
}

impl Profile {
    /// Parse and validate a profile file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, ProfileError> {
        let v = strict_json::parse(bytes).map_err(|e| ProfileError::Shape(e.to_string()))?;
        let w: ProfileWire =
            serde_json::from_value(v).map_err(|e| ProfileError::Shape(e.to_string()))?;
        let f = ProfileError::Field;
        if !PROFILE_VERSIONS.contains(&w.profile_version) {
            return Err(f("profile_version"));
        }
        if !is_id(&w.id) {
            return Err(f("id"));
        }
        if !is_model_name(&w.model) {
            return Err(f("model"));
        }
        if !(512..=4_194_304).contains(&w.context_window) {
            return Err(f("context_window"));
        }
        if !(w.fill_ratio > 0.0 && w.fill_ratio <= 1.0) {
            return Err(f("fill_ratio"));
        }
        if !(5..=8).contains(&w.max_active_tools) {
            return Err(f("max_active_tools"));
        }
        if !(1..=32).contains(&w.recent_turns) {
            return Err(f("recent_turns"));
        }
        let s = w.sampling;
        if !(0.0..=2.0).contains(&s.temperature) {
            return Err(f("sampling.temperature"));
        }
        if !(s.top_p > 0.0 && s.top_p <= 1.0) {
            return Err(f("sampling.top_p"));
        }
        if s.max_tokens == 0 || s.max_tokens >= w.context_window {
            return Err(f("sampling.max_tokens"));
        }
        if let Some(n) = &w.kv_quant_note {
            if n.is_empty() || n.len() > 256 || n.chars().any(char::is_control) {
                return Err(f("kv_quant_note"));
            }
        }
        if let Some(st) = &w.validated {
            if st.report_sha256.parse::<Digest>().is_err() {
                return Err(f("validated.report_sha256"));
            }
        }
        if w.grammar != Grammar::None {
            return Err(ProfileError::NotInThisBuild {
                field: "grammar",
                why: "constrained decoding is not sent by this build (spike S-P1)",
            });
        }
        if w.price.is_some() {
            return Err(ProfileError::NotInThisBuild {
                field: "price",
                why: "prices apply to hosted endpoints, which this build does not have",
            });
        }
        if w.tool_choice_required_ok && w.protocol == Protocol::Text {
            return Err(f("tool_choice_required_ok"));
        }
        Ok(Self {
            id: w.id,
            model: w.model,
            context_window: w.context_window,
            fill_ratio: w.fill_ratio,
            protocol: w.protocol,
            tool_choice_required_ok: w.tool_choice_required_ok,
            max_active_tools: w.max_active_tools,
            edit_format: w.edit_format,
            recent_turns: w.recent_turns,
            sampling: s,
            kv_quant_note: w.kv_quant_note,
            validated: w.validated,
            sha256: Some(sha256(bytes)),
        })
    }

    /// The conservative default for a model without a profile (§3.4): text
    /// protocol, 5 tools, replace edits, K = 4. Unvalidated.
    pub fn conservative_default(model: &str) -> Self {
        Self {
            id: "default".into(),
            model: model.to_owned(),
            context_window: 8192,
            fill_ratio: 0.6,
            protocol: Protocol::Text,
            tool_choice_required_ok: false,
            max_active_tools: 5,
            edit_format: EditFormat::Replace,
            recent_turns: 4,
            sampling: Sampling {
                temperature: 0.2,
                top_p: 0.95,
                seed: None,
                max_tokens: 1024,
            },
            kv_quant_note: None,
            validated: None,
            sha256: None,
        }
    }

    /// Profile id.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The server's model name.
    pub fn model(&self) -> &str {
        &self.model
    }
    /// Context window in tokens.
    pub fn context_window(&self) -> u64 {
        self.context_window
    }
    /// Context fill ratio (§2.3).
    pub fn fill_ratio(&self) -> f64 {
        self.fill_ratio
    }
    /// Action protocol.
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }
    /// Whether `tool_choice: "required"` may be sent.
    pub fn tool_choice_required_ok(&self) -> bool {
        self.tool_choice_required_ok
    }
    /// Cap on the active tool set.
    pub fn max_active_tools(&self) -> u32 {
        self.max_active_tools
    }
    /// Edit format.
    pub fn edit_format(&self) -> EditFormat {
        self.edit_format
    }
    /// Recent turns kept verbatim (K).
    pub fn recent_turns(&self) -> u32 {
        self.recent_turns
    }
    /// Sampling defaults.
    pub fn sampling(&self) -> Sampling {
        self.sampling
    }
    /// The KV-quantization note.
    pub fn kv_quant_note(&self) -> Option<&str> {
        self.kv_quant_note.as_deref()
    }
    /// Whether `profile check` stamped this profile. Recorded as
    /// `profile_validated` in every journal header (§3.4).
    pub fn validated(&self) -> bool {
        self.validated.is_some()
    }
    /// SHA-256 of the profile file, when loaded from one.
    pub fn sha256(&self) -> Option<Digest> {
        self.sha256
    }
}

/// What the `profile check` smoke eval observed (§3.4). Edit-format
/// compliance needs the edit tools (H2) and is recorded as unchecked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmokeResults {
    /// Cases run.
    pub cases: u32,
    /// Replies that parsed to exactly the expected tool call.
    pub valid_tool_calls: u32,
    /// Replies that were format errors.
    pub format_errors: u32,
    /// Model calls that failed outright (transport or typed model error).
    pub call_failures: u32,
}

/// The verdict of `profile check`: stamp the profile, or not. Not a gate
/// outcome (it describes a profile, not a run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// Passed; stamp the profile with this digest.
    Stamp(Stamp),
    /// Did not pass; the reason.
    NoStamp(&'static str),
}

/// Minimum smoke cases for a stamp.
pub const SMOKE_MIN_CASES: u32 = 5;

/// Score smoke results (pure). A stamp needs at least [`SMOKE_MIN_CASES`]
/// cases, no failed call, at least 80% exactly valid tool calls, and a
/// format-error rate of at most 20%. The stamp is the SHA-256 of a
/// canonical report line naming the profile, the counts and the unchecked
/// edit-format part.
pub fn score(profile: &Profile, r: &SmokeResults) -> CheckResult {
    if r.cases < SMOKE_MIN_CASES {
        return CheckResult::NoStamp("too few cases");
    }
    if r.call_failures > 0 {
        return CheckResult::NoStamp("a model call failed");
    }
    if u64::from(r.valid_tool_calls) * 5 < u64::from(r.cases) * 4 {
        return CheckResult::NoStamp("fewer than 80% valid tool calls");
    }
    if u64::from(r.format_errors) * 5 > u64::from(r.cases) {
        return CheckResult::NoStamp("format-error rate above 20%");
    }
    let report = format!(
        "profile-check/1 id={} model={} protocol={:?} cases={} valid={} format_errors={} edit_format=unchecked",
        profile.id, profile.model, profile.protocol, r.cases, r.valid_tool_calls, r.format_errors
    );
    CheckResult::Stamp(Stamp {
        report_sha256: sha256(report.as_bytes()).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"profile_version":1,"id":"qwen-7b-q4","model":"qwen2.5-coder:7b",
      "context_window":32768,"fill_ratio":0.6,"protocol":"text","tool_choice_required_ok":false,
      "grammar":"none","max_active_tools":6,"edit_format":"replace","recent_turns":5,
      "sampling":{"temperature":0.2,"top_p":0.95,"seed":7,"max_tokens":2048}}"#;

    fn with(k: &str, v: &str) -> String {
        let mut o: serde_json::Map<String, serde_json::Value> = serde_json::from_str(GOOD).unwrap();
        o.insert(k.into(), serde_json::from_str(v).unwrap());
        serde_json::Value::Object(o).to_string()
    }

    #[test]
    fn good_profile_parses_unvalidated() {
        let p = Profile::parse(GOOD.as_bytes()).unwrap();
        assert_eq!(p.protocol(), Protocol::Text);
        assert!(!p.validated());
        assert_eq!(p.sha256(), Some(sha256(GOOD.as_bytes())));
    }

    #[test]
    fn conservative_default_matches_the_design() {
        let p = Profile::conservative_default("anything");
        assert_eq!(p.protocol(), Protocol::Text);
        assert_eq!(p.max_active_tools(), 5);
        assert_eq!(p.edit_format(), EditFormat::Replace);
        assert_eq!(p.recent_turns(), 4);
        assert!(!p.validated());
    }

    #[test]
    fn bad_profiles_are_refused() {
        let dup = GOOD.replacen("\"id\":\"qwen-7b-q4\"", "\"id\":\"a\",\"id\":\"b\"", 1);
        assert!(matches!(
            Profile::parse(dup.as_bytes()),
            Err(ProfileError::Shape(_))
        ));
        for (k, v) in [("surprise", "1"), ("protocol", "\"xml\"")] {
            assert!(
                matches!(
                    Profile::parse(with(k, v).as_bytes()),
                    Err(ProfileError::Shape(_))
                ),
                "{k}"
            );
        }
        for (k, v) in [
            ("profile_version", "2"),
            ("id", "\"a b\""),
            ("model", "\"\""),
            ("context_window", "100"),
            ("fill_ratio", "0"),
            ("fill_ratio", "1.5"),
            ("max_active_tools", "9"),
            ("max_active_tools", "4"),
            ("recent_turns", "0"),
            (
                "sampling",
                r#"{"temperature":3,"top_p":0.9,"max_tokens":10}"#,
            ),
            (
                "sampling",
                r#"{"temperature":0.2,"top_p":0,"max_tokens":10}"#,
            ),
            (
                "sampling",
                r#"{"temperature":0.2,"top_p":0.9,"max_tokens":40000}"#,
            ),
            ("kv_quant_note", "\"a\\nb\""),
            ("validated", r#"{"report_sha256":"nope"}"#),
        ] {
            assert!(
                matches!(
                    Profile::parse(with(k, v).as_bytes()),
                    Err(ProfileError::Field(_))
                ),
                "{k}={v}"
            );
        }
        let native_tc = with("tool_choice_required_ok", "true");
        assert_eq!(
            Profile::parse(native_tc.as_bytes()),
            Err(ProfileError::Field("tool_choice_required_ok"))
        );
        for (k, v) in [("grammar", "\"gbnf_lazy\""), ("price", r#"{"input":1}"#)] {
            assert!(
                matches!(
                    Profile::parse(with(k, v).as_bytes()),
                    Err(ProfileError::NotInThisBuild { .. })
                ),
                "{k}"
            );
        }
    }

    #[test]
    fn a_stamped_profile_reports_validated() {
        let stamped = with(
            "validated",
            r#"{"report_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}"#,
        );
        assert!(Profile::parse(stamped.as_bytes()).unwrap().validated());
    }

    #[test]
    fn smoke_scoring_is_strict() {
        let p = Profile::conservative_default("m");
        let r = |cases, valid, fe, fail| SmokeResults {
            cases,
            valid_tool_calls: valid,
            format_errors: fe,
            call_failures: fail,
        };
        assert!(matches!(score(&p, &r(5, 5, 0, 0)), CheckResult::Stamp(_)));
        assert!(matches!(score(&p, &r(5, 4, 1, 0)), CheckResult::Stamp(_)));
        assert!(matches!(score(&p, &r(4, 4, 0, 0)), CheckResult::NoStamp(_)));
        assert!(matches!(score(&p, &r(5, 3, 2, 0)), CheckResult::NoStamp(_)));
        assert!(matches!(score(&p, &r(5, 5, 0, 1)), CheckResult::NoStamp(_)));
        // Deterministic stamp.
        assert_eq!(score(&p, &r(5, 5, 0, 0)), score(&p, &r(5, 5, 0, 0)));
    }
}
