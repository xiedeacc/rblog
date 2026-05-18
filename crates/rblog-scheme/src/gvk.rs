//! `GroupVersionKind` — the K8s-style identifier of every Extension kind.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Group/Version/Kind triple, with the K8s convention that an empty `group`
/// is the "core" group (Halo treats `User`, `Setting`, `ConfigMap`, etc. as core).
///
/// This type is `Copy` and uses `&'static str` for the parts — concrete kinds
/// declare their GVK as a `const`.
///
/// ```
/// use rblog_scheme::GroupVersionKind;
///
/// const POST_GVK: GroupVersionKind = GroupVersionKind::new(
///     "content.halo.run",
///     "v1alpha1",
///     "Post",
///     "posts",
///     "post",
/// );
///
/// assert_eq!(POST_GVK.api_version(), "content.halo.run/v1alpha1");
/// assert_eq!(POST_GVK.kind, "Post");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupVersionKind {
    /// The group, e.g. `content.halo.run`. May be empty for core kinds.
    pub group: &'static str,
    /// The version, e.g. `v1alpha1`.
    pub version: &'static str,
    /// The kind in PascalCase, e.g. `Post`.
    pub kind: &'static str,
    /// The plural lowercased noun used in the store path, e.g. `posts`.
    pub plural: &'static str,
    /// The singular lowercased noun, e.g. `post`. Currently kept for parity with
    /// Halo's annotation but unused in storage.
    pub singular: &'static str,
}

impl GroupVersionKind {
    #[must_use]
    pub const fn new(
        group: &'static str,
        version: &'static str,
        kind: &'static str,
        plural: &'static str,
        singular: &'static str,
    ) -> Self {
        Self {
            group,
            version,
            kind,
            plural,
            singular,
        }
    }

    /// Wire-format `apiVersion`: `<group>/<version>`, or just `<version>` when
    /// the group is empty.
    ///
    /// Matches Halo's behaviour: a `User`'s `apiVersion` is `"v1alpha1"`, a
    /// `Post`'s is `"content.halo.run/v1alpha1"`.
    #[must_use]
    pub fn api_version(&self) -> String {
        if self.group.is_empty() {
            self.version.to_owned()
        } else {
            format!("{}/{}", self.group, self.version)
        }
    }

    /// Whether this GVK belongs to the "core" (empty) group.
    #[must_use]
    pub const fn is_core(&self) -> bool {
        self.group.is_empty()
    }
}

impl fmt::Display for GroupVersionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_core() {
            write!(f, "{}/{}", self.version, self.kind)
        } else {
            write!(f, "{}/{}/{}", self.group, self.version, self.kind)
        }
    }
}

/// Owned variant used when GVKs are constructed at runtime (e.g. by plugins
/// declaring their own kinds, or when deserializing a GVK over the wire).
///
/// The static [`GroupVersionKind`] is preferred for built-in kinds since it
/// is `Copy` and lives in the binary's data section.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(dead_code, unreachable_pub)] // used by rblog-plugins in a follow-up PR
pub struct OwnedGvk {
    pub group: String,
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub singular: String,
}

impl From<GroupVersionKind> for OwnedGvk {
    fn from(g: GroupVersionKind) -> Self {
        Self {
            group: g.group.to_owned(),
            version: g.version.to_owned(),
            kind: g.kind.to_owned(),
            plural: g.plural.to_owned(),
            singular: g.singular.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POST_GVK: GroupVersionKind =
        GroupVersionKind::new("content.halo.run", "v1alpha1", "Post", "posts", "post");

    const USER_GVK: GroupVersionKind =
        GroupVersionKind::new("", "v1alpha1", "User", "users", "user");

    #[test]
    fn api_version_grouped() {
        assert_eq!(POST_GVK.api_version(), "content.halo.run/v1alpha1");
    }

    #[test]
    fn api_version_core() {
        assert_eq!(USER_GVK.api_version(), "v1alpha1");
        assert!(USER_GVK.is_core());
    }

    #[test]
    fn display() {
        assert_eq!(POST_GVK.to_string(), "content.halo.run/v1alpha1/Post");
        assert_eq!(USER_GVK.to_string(), "v1alpha1/User");
    }
}
