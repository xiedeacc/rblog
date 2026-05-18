//! Halo-compatible line-based patch model.
//!
//! Halo persists post content as snapshot deltas using a line-based diff
//! (powered by java-diff-utils). The JSON representation is an array of
//! delta objects:
//!
//! ```json
//! [
//!   {
//!     "source": { "position": 0, "lines": ["a"], "changePosition": null },
//!     "target": { "position": 0, "lines": ["A"], "changePosition": null },
//!     "type": "CHANGE"
//!   }
//! ]
//! ```
//!
//! This module mirrors that format byte-for-byte so Halo-produced
//! `Snapshot.spec.rawPatch` / `contentPatch` payloads apply cleanly here, and
//! patches we produce apply cleanly under Halo. The actual diff algorithm is
//! [Myers] line-diff via the [`similar`] crate, packaged into Halo's chunked
//! delta shape.
//!
//! [Myers]: https://en.wikipedia.org/wiki/Diff#The_Myers_algorithm

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("invalid patch JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("patch failed to apply at line {position} (kind={kind:?})")]
    Apply { position: usize, kind: DeltaType },
}

/// Line-based delta kind. Serializes as Halo's `DeltaType` enum names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum DeltaType {
    Insert,
    Delete,
    Change,
}

/// One chunk inside a `Delta`. Mirrors Halo's `PatchUtils.StringChunk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringChunk {
    pub position: usize,
    pub lines: Vec<String>,
    /// `changePosition` in Halo is unused for our purposes — always emitted
    /// as `null`. We preserve it as `Option<Vec<usize>>` for compatibility on
    /// the read side.
    #[serde(
        default,
        rename = "changePosition",
        skip_serializing_if = "Option::is_none"
    )]
    pub change_position: Option<Vec<usize>>,
}

/// One delta: a paired source+target chunk and a kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub source: StringChunk,
    pub target: StringChunk,
    #[serde(rename = "type")]
    pub kind: DeltaType,
}

/// Apply a Halo-format JSON patch to `original`. Returns the patched string.
pub fn apply_patch(original: &str, patch_json: &str) -> Result<String, PatchError> {
    let deltas: Vec<Delta> = if patch_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(patch_json)?
    };
    apply_deltas(original, &deltas)
}

/// Apply parsed deltas to `original`. Useful when you already have the
/// deserialized representation.
pub fn apply_deltas(original: &str, deltas: &[Delta]) -> Result<String, PatchError> {
    let mut original_lines: Vec<&str> = if original.is_empty() {
        Vec::new()
    } else {
        original.split('\n').collect()
    };

    // We must apply in source-position order (Halo asserts the patch is
    // ordered as produced by DiffUtils.diff). We rebuild the output by
    // walking original lines and intercepting at each delta's source.
    let mut deltas = deltas.to_vec();
    deltas.sort_by_key(|d| d.source.position);

    let mut output: Vec<String> = Vec::with_capacity(original_lines.len());
    let mut cursor = 0usize;
    for delta in &deltas {
        let pos = delta.source.position;
        if pos > original_lines.len() {
            return Err(PatchError::Apply {
                position: pos,
                kind: delta.kind,
            });
        }
        while cursor < pos {
            output.push(original_lines[cursor].to_owned());
            cursor += 1;
        }
        match delta.kind {
            DeltaType::Insert => {
                output.extend(delta.target.lines.iter().cloned());
            }
            DeltaType::Delete => {
                if cursor + delta.source.lines.len() > original_lines.len() {
                    return Err(PatchError::Apply {
                        position: pos,
                        kind: delta.kind,
                    });
                }
                cursor += delta.source.lines.len();
            }
            DeltaType::Change => {
                if cursor + delta.source.lines.len() > original_lines.len() {
                    return Err(PatchError::Apply {
                        position: pos,
                        kind: delta.kind,
                    });
                }
                cursor += delta.source.lines.len();
                output.extend(delta.target.lines.iter().cloned());
            }
        }
    }
    while cursor < original_lines.len() {
        output.push(original_lines[cursor].to_owned());
        cursor += 1;
    }
    // Re-drop the temporary borrow so it's clear we don't need it any more.
    original_lines.clear();
    Ok(output.join("\n"))
}

/// Diff `original` against `revised` and return the Halo-format patch JSON.
pub fn diff_to_json_patch(original: &str, revised: &str) -> Result<String, PatchError> {
    let deltas = diff(original, revised);
    Ok(serde_json::to_string(&deltas)?)
}

/// Diff `original` against `revised` returning [`Delta`]s. Useful when you
/// want to inspect the structure without going through JSON.
#[must_use]
pub fn diff(original: &str, revised: &str) -> Vec<Delta> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(original, revised);
    let mut deltas: Vec<Delta> = Vec::new();

    let mut src_pos = 0usize;
    let mut tgt_pos = 0usize;
    let mut buf_del: Vec<String> = Vec::new();
    let mut buf_ins: Vec<String> = Vec::new();
    let mut block_src_start: usize = 0;
    let mut block_tgt_start: usize = 0;

    let flush = |deltas: &mut Vec<Delta>,
                 src_start: usize,
                 tgt_start: usize,
                 dels: &mut Vec<String>,
                 inss: &mut Vec<String>| {
        if dels.is_empty() && inss.is_empty() {
            return;
        }
        let kind = match (dels.is_empty(), inss.is_empty()) {
            (false, false) => DeltaType::Change,
            (true, false) => DeltaType::Insert,
            (false, true) => DeltaType::Delete,
            (true, true) => unreachable!(),
        };
        deltas.push(Delta {
            source: StringChunk {
                position: src_start,
                lines: std::mem::take(dels),
                change_position: None,
            },
            target: StringChunk {
                position: tgt_start,
                lines: std::mem::take(inss),
                change_position: None,
            },
            kind,
        });
    };

    let mut in_block = false;
    for change in diff.iter_all_changes() {
        let value = strip_trailing_newline(change.value());
        match change.tag() {
            ChangeTag::Equal => {
                if in_block {
                    flush(
                        &mut deltas,
                        block_src_start,
                        block_tgt_start,
                        &mut buf_del,
                        &mut buf_ins,
                    );
                    in_block = false;
                }
                src_pos += 1;
                tgt_pos += 1;
            }
            ChangeTag::Delete => {
                if !in_block {
                    block_src_start = src_pos;
                    block_tgt_start = tgt_pos;
                    in_block = true;
                }
                buf_del.push(value.to_owned());
                src_pos += 1;
            }
            ChangeTag::Insert => {
                if !in_block {
                    block_src_start = src_pos;
                    block_tgt_start = tgt_pos;
                    in_block = true;
                }
                buf_ins.push(value.to_owned());
                tgt_pos += 1;
            }
        }
    }
    if in_block {
        flush(
            &mut deltas,
            block_src_start,
            block_tgt_start,
            &mut buf_del,
            &mut buf_ins,
        );
    }
    deltas
}

fn strip_trailing_newline(s: &str) -> &str {
    s.strip_suffix('\n').unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn insert_delta_serializes_uppercase_type() {
        let d = Delta {
            source: StringChunk {
                position: 0,
                lines: vec![],
                change_position: None,
            },
            target: StringChunk {
                position: 0,
                lines: vec!["hello".to_owned()],
                change_position: None,
            },
            kind: DeltaType::Insert,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["type"], "INSERT");
        assert_eq!(json["source"]["position"], 0);
        assert_eq!(json["target"]["lines"][0], "hello");
        assert!(json["source"]
            .as_object()
            .unwrap()
            .get("changePosition")
            .is_none());
    }

    #[test]
    fn diff_then_apply_round_trips() {
        let original = "alpha\nbeta\ngamma";
        let revised = "alpha\nBETA\ngamma\ndelta";
        let patch = diff_to_json_patch(original, revised).unwrap();
        let applied = apply_patch(original, &patch).unwrap();
        assert_eq!(applied, revised);
    }

    #[test]
    fn applying_empty_patch_returns_original() {
        let s = "one\ntwo\nthree";
        assert_eq!(apply_patch(s, "[]").unwrap(), s);
        assert_eq!(apply_patch(s, "   ").unwrap(), s);
    }

    #[test]
    fn applying_to_empty_string_yields_target_lines() {
        let patch = diff_to_json_patch("", "hello").unwrap();
        assert_eq!(apply_patch("", &patch).unwrap(), "hello");
    }

    #[test]
    fn change_delta_replaces_lines() {
        let original = "one\ntwo\nthree";
        let revised = "one\nTWO\nthree";
        let patch = diff_to_json_patch(original, revised).unwrap();
        assert_eq!(apply_patch(original, &patch).unwrap(), revised);
    }

    #[test]
    fn delete_only_block_shrinks() {
        let original = "a\nb\nc";
        let revised = "a\nc";
        let patch = diff_to_json_patch(original, revised).unwrap();
        assert_eq!(apply_patch(original, &patch).unwrap(), revised);
    }

    #[test]
    fn halo_shape_patch_applies() {
        // Hand-rolled patch in Halo's JSON shape.
        let halo_patch = r#"[
          {
            "source": { "position": 1, "lines": ["beta"], "changePosition": null },
            "target": { "position": 1, "lines": ["BETA", "Beta2"], "changePosition": null },
            "type": "CHANGE"
          }
        ]"#;
        let original = "alpha\nbeta\ngamma";
        let out = apply_patch(original, halo_patch).unwrap();
        assert_eq!(out, "alpha\nBETA\nBeta2\ngamma");
    }

    #[test]
    fn deserialize_halo_delta_with_null_change_position() {
        let raw = r#"{
          "source": { "position": 0, "lines": ["x"], "changePosition": null },
          "target": { "position": 0, "lines": ["y"], "changePosition": null },
          "type": "CHANGE"
        }"#;
        let d: Delta = serde_json::from_str(raw).unwrap();
        assert_eq!(d.kind, DeltaType::Change);
        assert_eq!(d.source.change_position, None);
    }

    #[test]
    fn out_of_range_position_returns_error() {
        let patch = r#"[{
          "source": { "position": 99, "lines": ["x"], "changePosition": null },
          "target": { "position": 99, "lines": ["y"], "changePosition": null },
          "type": "CHANGE"
        }]"#;
        let err = apply_patch("a\nb", patch).expect_err("should fail");
        assert!(matches!(err, PatchError::Apply { .. }));
    }
}
