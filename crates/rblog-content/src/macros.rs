//! Internal helper macros.

/// Declares the top-level wrapper for a Halo Extension kind.
///
/// Given:
///
/// - `$Name`   — the Rust struct name (e.g. `Post`).
/// - `$gvk`    — a `const GroupVersionKind` for the kind.
/// - `$Spec`   — the type of the `spec` field.
/// - `$Status` — the type of the `status` field.
///
/// Generates a struct with `api_version`, `kind`, `metadata`, `spec` and
/// `status` fields plus the `Extension` impl and a `new()` constructor that
/// pre-fills `api_version` / `kind` from the GVK.
///
/// To opt out of `spec` or `status`, pass an empty unit type wrapper like
/// `()` is *not* sufficient (serde would emit `null`); use a custom struct
/// or just don't call the macro and write the type out by hand. All current
/// Halo kinds have at least a `spec`.
macro_rules! define_kind {
    (
        $(#[$meta:meta])*
        $Name:ident,
        gvk = $gvk:expr,
        spec = $Spec:ty,
        status = $Status:ty $(,)?
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $Name {
            #[serde(default)]
            pub api_version: String,
            #[serde(default)]
            pub kind: String,
            #[serde(default)]
            pub metadata: ::rblog_scheme::Metadata,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub spec: Option<$Spec>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub status: Option<$Status>,
        }

        impl $Name {
            /// Build a default-shaped instance with `api_version` / `kind` set
            /// from the GVK and an empty [`rblog_scheme::Metadata`].
            #[must_use]
            pub fn new(name: impl Into<String>) -> Self {
                let gvk = <Self as ::rblog_scheme::Extension>::gvk();
                Self {
                    api_version: gvk.api_version(),
                    kind: gvk.kind.to_owned(),
                    metadata: ::rblog_scheme::Metadata::new(name),
                    spec: None,
                    status: None,
                }
            }

            /// Replace the `spec`.
            #[must_use]
            pub fn with_spec(mut self, spec: $Spec) -> Self {
                self.spec = Some(spec);
                self
            }
        }

        impl Default for $Name {
            fn default() -> Self {
                Self::new("")
            }
        }

        impl ::rblog_scheme::Extension for $Name {
            fn gvk() -> ::rblog_scheme::GroupVersionKind {
                $gvk
            }
            fn metadata(&self) -> &::rblog_scheme::Metadata {
                &self.metadata
            }
            fn metadata_mut(&mut self) -> &mut ::rblog_scheme::Metadata {
                &mut self.metadata
            }
        }
    };
}
