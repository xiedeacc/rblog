//! Store-name construction & parsing for the `extensions` table.
//!
//! Names follow Halo's exact rule (see `ExtensionStoreUtil.buildStoreName`):
//!
//! ```text
//! /registry/<group>/<plural>/<name>     -- when group is non-empty
//! /registry/<plural>/<name>             -- when group is empty
//! ```

use thiserror::Error;

use crate::GroupVersionKind;

/// Build the prefix for a kind, **without** the trailing slash.
///
/// ```
/// use rblog_scheme::{build_store_name_prefix, GroupVersionKind};
///
/// const POST: GroupVersionKind =
///     GroupVersionKind::new("content.halo.run", "v1alpha1", "Post", "posts", "post");
/// const USER: GroupVersionKind =
///     GroupVersionKind::new("", "v1alpha1", "User", "users", "user");
///
/// assert_eq!(build_store_name_prefix(&POST), "/registry/content.halo.run/posts");
/// assert_eq!(build_store_name_prefix(&USER), "/registry/users");
/// ```
#[must_use]
pub fn build_store_name_prefix(gvk: &GroupVersionKind) -> String {
    if gvk.is_core() {
        format!("/registry/{}", gvk.plural)
    } else {
        format!("/registry/{}/{}", gvk.group, gvk.plural)
    }
}

/// Build the full store name for a single Extension instance.
///
/// ```
/// use rblog_scheme::{build_store_name, GroupVersionKind};
///
/// const POST: GroupVersionKind =
///     GroupVersionKind::new("content.halo.run", "v1alpha1", "Post", "posts", "post");
///
/// assert_eq!(
///     build_store_name(&POST, "my-first-post"),
///     "/registry/content.halo.run/posts/my-first-post"
/// );
/// ```
#[must_use]
pub fn build_store_name(gvk: &GroupVersionKind, name: &str) -> String {
    format!("{}/{}", build_store_name_prefix(gvk), name)
}

/// Errors returned by [`parse_store_name`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreNameError {
    #[error("store name must start with `/registry/`, got {0:?}")]
    MissingRegistryPrefix(String),
    #[error("store name has too few path segments: {0:?}")]
    TooShort(String),
    #[error("store name has an empty segment: {0:?}")]
    EmptySegment(String),
}

/// Parsed store-name components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStoreName {
    /// May be empty for core kinds.
    pub group: String,
    pub plural: String,
    pub name: String,
}

/// Parse a store name back into its `(group, plural, name)` parts.
///
/// This is the inverse of [`build_store_name`] for the two valid shapes
/// described above. Useful for round-trip tests and for the index engine when
/// reconciling raw rows fetched by prefix.
///
/// ```
/// use rblog_scheme::parse_store_name;
///
/// let parsed = parse_store_name("/registry/content.halo.run/posts/hello").unwrap();
/// assert_eq!(parsed.group, "content.halo.run");
/// assert_eq!(parsed.plural, "posts");
/// assert_eq!(parsed.name, "hello");
///
/// let parsed = parse_store_name("/registry/users/admin").unwrap();
/// assert_eq!(parsed.group, "");
/// assert_eq!(parsed.plural, "users");
/// assert_eq!(parsed.name, "admin");
/// ```
pub fn parse_store_name(input: &str) -> Result<ParsedStoreName, StoreNameError> {
    let rest = input
        .strip_prefix("/registry/")
        .ok_or_else(|| StoreNameError::MissingRegistryPrefix(input.to_owned()))?;
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(StoreNameError::EmptySegment(input.to_owned()));
    }
    match segments.as_slice() {
        [plural, name] => Ok(ParsedStoreName {
            group: String::new(),
            plural: (*plural).to_owned(),
            name: (*name).to_owned(),
        }),
        [group, plural, name] => Ok(ParsedStoreName {
            group: (*group).to_owned(),
            plural: (*plural).to_owned(),
            name: (*name).to_owned(),
        }),
        _ => Err(StoreNameError::TooShort(input.to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const POST: GroupVersionKind =
        GroupVersionKind::new("content.halo.run", "v1alpha1", "Post", "posts", "post");
    const USER: GroupVersionKind = GroupVersionKind::new("", "v1alpha1", "User", "users", "user");

    #[test]
    fn prefix_grouped() {
        assert_eq!(
            build_store_name_prefix(&POST),
            "/registry/content.halo.run/posts"
        );
    }

    #[test]
    fn prefix_core() {
        assert_eq!(build_store_name_prefix(&USER), "/registry/users");
    }

    #[test]
    fn build_grouped() {
        assert_eq!(
            build_store_name(&POST, "hello-world"),
            "/registry/content.halo.run/posts/hello-world"
        );
    }

    #[test]
    fn build_core() {
        assert_eq!(build_store_name(&USER, "admin"), "/registry/users/admin");
    }

    #[test]
    fn parse_grouped() {
        let p = parse_store_name("/registry/content.halo.run/posts/hello").unwrap();
        assert_eq!(p.group, "content.halo.run");
        assert_eq!(p.plural, "posts");
        assert_eq!(p.name, "hello");
    }

    #[test]
    fn parse_core() {
        let p = parse_store_name("/registry/users/admin").unwrap();
        assert_eq!(p.group, "");
        assert_eq!(p.plural, "users");
        assert_eq!(p.name, "admin");
    }

    #[test]
    fn parse_round_trip() {
        for input in [
            "/registry/content.halo.run/posts/hello",
            "/registry/storage.halo.run/attachments/img-1",
            "/registry/users/admin",
            "/registry/menus/main",
        ] {
            let parsed = parse_store_name(input).unwrap();
            let rebuilt = if parsed.group.is_empty() {
                format!("/registry/{}/{}", parsed.plural, parsed.name)
            } else {
                format!(
                    "/registry/{}/{}/{}",
                    parsed.group, parsed.plural, parsed.name
                )
            };
            assert_eq!(rebuilt, input, "round-trip failed for {input}");
        }
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        let e = parse_store_name("content.halo.run/posts/x").unwrap_err();
        assert!(matches!(e, StoreNameError::MissingRegistryPrefix(_)));
    }

    #[test]
    fn parse_rejects_short() {
        let e = parse_store_name("/registry/users").unwrap_err();
        assert!(matches!(e, StoreNameError::TooShort(_)));
    }

    #[test]
    fn parse_rejects_empty_segment() {
        let e = parse_store_name("/registry/users//").unwrap_err();
        assert!(matches!(e, StoreNameError::EmptySegment(_)));
    }
}
