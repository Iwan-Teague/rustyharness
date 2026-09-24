//! The built-in manifest for H1's read tools (design §4.8, §9 H1: read tools
//! in-process, no exec).
//!
//! The built-ins are declared by a `harness` manifest and take the same
//! validation and policy path as any provider, with no special case beyond
//! the origin: this text is compiled in, so it alone may use the reserved
//! `harness` namespace and the `builtin` transport.
//!
//! H1 declares only the three read tools. `harness.edit.*`, `harness.exec.run`,
//! `harness.notes.write` and `harness.task.submit` are write/execute class and
//! arrive with the slices that implement their policy (H1e for the submit
//! sentinel and notes, H2 for edits and exec).
//!
//! Labels: read / operational / own / none, as §4.8's table says; `content`
//! is `third_party` because file contents in a workspace are other people's
//! text by default (§5.4). That changes no default decision (§5.2 allows both
//! read rows) and only makes the trifecta label honest.

use crate::{parse_with_origin, Manifest, ManifestError, Origin, ValidationContext};

/// The compiled-in manifest text (JSON, manifest v1).
pub const BUILTIN_MANIFEST_JSON: &str = r#"{
  "schema_version": 1,
  "provider": "harness",
  "provider_version": "0.0.1",
  "min_harness": "0.0.1",
  "transport": { "kind": "builtin" },
  "capabilities": [
    {
      "id": "harness.fs.read",
      "summary": "Read a window of lines from a file inside the workspace",
      "effect": "read",
      "sensitivity": "operational",
      "blast_radius": "own",
      "egress": "none",
      "content": "third_party",
      "confirmation": "none",
      "input_schema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "maxLength": 4096 },
          "start": { "type": "integer", "minimum": 1 },
          "lines": { "type": "integer", "minimum": 1, "maximum": 100 }
        },
        "required": ["path"]
      }
    },
    {
      "id": "harness.fs.search",
      "summary": "Search files inside the workspace with a regular expression",
      "effect": "read",
      "sensitivity": "operational",
      "blast_radius": "own",
      "egress": "none",
      "content": "third_party",
      "confirmation": "none",
      "input_schema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "pattern": { "type": "string", "maxLength": 1024 },
          "path": { "type": "string", "maxLength": 4096 }
        },
        "required": ["pattern"]
      }
    },
    {
      "id": "harness.fs.list",
      "summary": "List a directory inside the workspace, bounded in depth and count",
      "effect": "read",
      "sensitivity": "operational",
      "blast_radius": "own",
      "egress": "none",
      "content": "third_party",
      "confirmation": "none",
      "input_schema": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "path": { "type": "string", "maxLength": 4096 },
          "depth": { "type": "integer", "minimum": 1, "maximum": 4 }
        },
        "required": ["path"]
      }
    }
  ]
}"#;

/// The built-in manifest, validated like any other (plus: it is the only
/// manifest allowed the `harness` namespace and the `builtin` transport).
pub fn manifest(ctx: &ValidationContext) -> Result<Manifest, ManifestError> {
    parse_with_origin(BUILTIN_MANIFEST_JSON.as_bytes(), ctx, Origin::Compiled)
}
