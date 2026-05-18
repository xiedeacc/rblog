//! Snapshot patch composition: turn (head, base) snapshots into a single
//! materialized [`ContentWrapper`] holding the full raw + rendered content.
//!
//! Halo's content model:
//!
//! - The first snapshot of a post (the *base snapshot*) carries the full raw
//!   text and rendered HTML directly in `spec.rawPatch` / `spec.contentPatch`.
//!   Halo marks it with `metadata.annotations["content.halo.run/keep-raw"] =
//!   "true"`.
//! - Every subsequent snapshot stores a *unified diff* relative to the base
//!   in `spec.rawPatch` / `spec.contentPatch` (Halo format, see [`crate::patch`]).
//!
//! `compose_snapshot(head, base)` returns the materialized content. When
//! `head` is the base snapshot itself, no patch is applied.

use thiserror::Error;

use crate::content::Snapshot;
use crate::patch::{apply_patch, PatchError};

pub const KEEP_RAW_ANNOTATION: &str = "content.halo.run/keep-raw";

#[derive(Debug, Error)]
pub enum ContentWrapperError {
    #[error("base snapshot is missing required spec")]
    BaseMissingSpec,
    #[error("head snapshot is missing required spec")]
    HeadMissingSpec,
    #[error("base snapshot `{name}` is not annotated as a base (keep-raw=true)")]
    NotBase { name: String },
    #[error("patch failed: {0}")]
    Patch(#[from] PatchError),
}

/// Fully materialized post content, equivalent to Halo's `ContentWrapper`.
#[derive(Debug, Clone)]
pub struct ContentWrapper {
    pub snapshot_name: String,
    pub raw: String,
    pub content: String,
    pub raw_type: String,
}

/// Check whether `snap` is annotated as a base snapshot.
#[must_use]
pub fn is_base_snapshot(snap: &Snapshot) -> bool {
    snap.metadata
        .annotation(KEEP_RAW_ANNOTATION)
        .is_some_and(|v| v == "true")
}

/// Compose the full content for `head` relative to `base`.
///
/// Returns a [`ContentWrapper`] holding the materialized `raw` and `content`
/// strings. When `head` *is* the base snapshot, the wrapper just unwraps
/// `base.spec.{rawPatch,contentPatch}` verbatim.
///
/// # Errors
///
/// - Returns [`ContentWrapperError::NotBase`] if the given base does not have
///   the `content.halo.run/keep-raw` annotation set to `"true"`.
/// - Returns [`ContentWrapperError::Patch`] if applying the head's diff to
///   the base fails.
pub fn compose_snapshot(
    head: &Snapshot,
    base: &Snapshot,
) -> Result<ContentWrapper, ContentWrapperError> {
    if !is_base_snapshot(base) {
        return Err(ContentWrapperError::NotBase {
            name: base.metadata.name().to_owned(),
        });
    }

    let base_spec = base
        .spec
        .as_ref()
        .ok_or(ContentWrapperError::BaseMissingSpec)?;
    let head_spec = head
        .spec
        .as_ref()
        .ok_or(ContentWrapperError::HeadMissingSpec)?;

    if head.metadata.name() == base.metadata.name() {
        return Ok(ContentWrapper {
            snapshot_name: head.metadata.name().to_owned(),
            raw: head_spec.raw_patch.clone().unwrap_or_default(),
            content: head_spec.content_patch.clone().unwrap_or_default(),
            raw_type: head_spec.raw_type.clone(),
        });
    }

    let base_raw = base_spec.raw_patch.as_deref().unwrap_or_default();
    let base_content = base_spec.content_patch.as_deref().unwrap_or_default();
    let head_raw_patch = head_spec.raw_patch.as_deref().unwrap_or("");
    let head_content_patch = head_spec.content_patch.as_deref().unwrap_or("");

    let raw = apply_patch(base_raw, head_raw_patch)?;
    let content = apply_patch(base_content, head_content_patch)?;

    Ok(ContentWrapper {
        snapshot_name: head.metadata.name().to_owned(),
        raw,
        content,
        raw_type: head_spec.raw_type.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Post, SnapshotSpec};
    use crate::infra::Ref;
    use crate::patch::diff_to_json_patch;
    use pretty_assertions::assert_eq;
    use rblog_scheme::Extension;

    fn make_base(name: &str, raw: &str, content: &str) -> Snapshot {
        let mut base = Snapshot::new(name).with_spec(SnapshotSpec {
            subject_ref: Ref::of_gvk("post", &Post::gvk()),
            raw_type: "markdown".to_owned(),
            raw_patch: Some(raw.to_owned()),
            content_patch: Some(content.to_owned()),
            parent_snapshot_name: None,
            last_modify_time: None,
            owner: "admin".to_owned(),
            contributors: None,
        });
        base.metadata.set_annotation(KEEP_RAW_ANNOTATION, "true");
        base
    }

    fn make_head(name: &str, base_name: &str, raw_patch: &str, content_patch: &str) -> Snapshot {
        Snapshot::new(name).with_spec(SnapshotSpec {
            subject_ref: Ref::of_gvk("post", &Post::gvk()),
            raw_type: "markdown".to_owned(),
            raw_patch: Some(raw_patch.to_owned()),
            content_patch: Some(content_patch.to_owned()),
            parent_snapshot_name: Some(base_name.to_owned()),
            last_modify_time: None,
            owner: "admin".to_owned(),
            contributors: None,
        })
    }

    #[test]
    fn head_equals_base_returns_base_verbatim() {
        let base = make_base("s1", "# Hello", "<h1>Hello</h1>");
        let wrap = compose_snapshot(&base, &base).expect("ok");
        assert_eq!(wrap.snapshot_name, "s1");
        assert_eq!(wrap.raw, "# Hello");
        assert_eq!(wrap.content, "<h1>Hello</h1>");
        assert_eq!(wrap.raw_type, "markdown");
    }

    #[test]
    fn head_diff_applies_to_base_raw_and_content() {
        let base_raw = "# Hello\n\nThis is the first draft.";
        let base_content = "<h1>Hello</h1>\n<p>This is the first draft.</p>";
        let new_raw = "# Hello, world!\n\nThis is the first draft.";
        let new_content = "<h1>Hello, world!</h1>\n<p>This is the first draft.</p>";
        let base = make_base("s-base", base_raw, base_content);
        let head_raw_patch = diff_to_json_patch(base_raw, new_raw).unwrap();
        let head_content_patch = diff_to_json_patch(base_content, new_content).unwrap();
        let head = make_head("s-head", "s-base", &head_raw_patch, &head_content_patch);
        let wrap = compose_snapshot(&head, &base).expect("ok");
        assert_eq!(wrap.snapshot_name, "s-head");
        assert_eq!(wrap.raw, new_raw);
        assert_eq!(wrap.content, new_content);
    }

    #[test]
    fn non_base_base_snapshot_returns_error() {
        let mut base = make_base("s1", "x", "y");
        base.metadata.remove_annotation(KEEP_RAW_ANNOTATION);
        let head = make_head("s2", "s1", "[]", "[]");
        let err = compose_snapshot(&head, &base).expect_err("must reject");
        assert!(matches!(err, ContentWrapperError::NotBase { .. }));
    }

    #[test]
    fn empty_head_patch_returns_base_text() {
        let base = make_base("b", "alpha\nbeta", "<p>alpha</p><p>beta</p>");
        let head = make_head("h", "b", "[]", "[]");
        let wrap = compose_snapshot(&head, &base).unwrap();
        assert_eq!(wrap.raw, "alpha\nbeta");
        assert_eq!(wrap.content, "<p>alpha</p><p>beta</p>");
    }
}
