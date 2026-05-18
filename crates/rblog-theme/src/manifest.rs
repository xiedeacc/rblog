//! `theme.yaml` schema, sliced from Halo's `Theme.spec`.
//!
//! We accept either YAML or JSON because both parse identically — `serde_yaml`
//! is a superset of JSON in practice. The fields mirror
//! `rblog_content::theme::ThemeSpec` so a Halo theme drops in without
//! modification.

use std::path::Path;

use rblog_content::theme::{CustomTemplates, ThemeSpec};
use rblog_content::Theme;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ThemeManifestError {
    #[error("read theme manifest at {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("parse theme manifest: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("theme manifest at {path} is missing `spec.displayName`")]
    MissingDisplayName { path: String },
    #[error("theme manifest at {path} is missing `metadata.name`")]
    MissingName { path: String },
}

/// In-memory representation of a parsed `theme.yaml`.
#[derive(Debug, Clone)]
pub struct ThemeManifest {
    pub name: String,
    pub spec: ThemeSpec,
}

impl ThemeManifest {
    /// Load a `theme.yaml` from `path`. Accepts the Halo wrapper shape
    /// (`apiVersion / kind / metadata / spec`) or a slimmed-down shape
    /// (`name + spec`) for hand-written themes.
    pub fn from_yaml_file(path: &Path) -> Result<Self, ThemeManifestError> {
        let body = std::fs::read_to_string(path).map_err(|e| ThemeManifestError::Read {
            path: path.display().to_string(),
            source: e,
        })?;
        Self::from_yaml_str(&body, &path.display().to_string())
    }

    /// Parse a YAML or JSON manifest body.
    pub fn from_yaml_str(body: &str, source: &str) -> Result<Self, ThemeManifestError> {
        // Try the full Halo Theme shape first (apiVersion/kind/metadata/spec).
        // If `metadata.name` is non-empty, we trust that path. Otherwise fall
        // back to the slim shape (`name + spec`) so hand-written themes work.
        if let Ok(theme) = serde_yaml::from_str::<Theme>(body) {
            if !theme.metadata.name().is_empty() {
                let name = theme.metadata.name().to_owned();
                let spec = theme
                    .spec
                    .ok_or_else(|| ThemeManifestError::MissingDisplayName {
                        path: source.to_owned(),
                    })?;
                if spec.display_name.is_empty() {
                    return Err(ThemeManifestError::MissingDisplayName {
                        path: source.to_owned(),
                    });
                }
                return Ok(Self { name, spec });
            }
        }
        let slim: SlimManifest = serde_yaml::from_str(body)?;
        if slim.name.is_empty() {
            return Err(ThemeManifestError::MissingName {
                path: source.to_owned(),
            });
        }
        if slim.spec.display_name.is_empty() {
            return Err(ThemeManifestError::MissingDisplayName {
                path: source.to_owned(),
            });
        }
        Ok(Self {
            name: slim.name,
            spec: slim.spec,
        })
    }

    /// Which template file (relative to `templates/`) should render a given
    /// post? Falls back to `post.html` if the post doesn't request a custom
    /// template.
    #[must_use]
    pub fn post_template(&self, requested: Option<&str>) -> String {
        if let Some(name) = requested {
            if let Some(td) = self
                .spec
                .custom_templates
                .as_ref()
                .and_then(CustomTemplates::resolve_post)
                .into_iter()
                .flatten()
                .find(|d| d.name == name)
            {
                return td.file.clone();
            }
        }
        "post.html".to_owned()
    }

    /// Same lookup for category pages.
    #[must_use]
    pub fn category_template(&self, requested: Option<&str>) -> String {
        if let Some(name) = requested {
            if let Some(td) = self
                .spec
                .custom_templates
                .as_ref()
                .and_then(CustomTemplates::resolve_category)
                .into_iter()
                .flatten()
                .find(|d| d.name == name)
            {
                return td.file.clone();
            }
        }
        "category.html".to_owned()
    }

    /// Same lookup for standalone single pages.
    #[must_use]
    pub fn single_page_template(&self, requested: Option<&str>) -> String {
        if let Some(name) = requested {
            if let Some(td) = self
                .spec
                .custom_templates
                .as_ref()
                .and_then(CustomTemplates::resolve_page)
                .into_iter()
                .flatten()
                .find(|d| d.name == name)
            {
                return td.file.clone();
            }
        }
        "page.html".to_owned()
    }
}

trait ResolveTemplates {
    fn resolve_post(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]>;
    fn resolve_category(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]>;
    fn resolve_page(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]>;
}

impl ResolveTemplates for CustomTemplates {
    fn resolve_post(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]> {
        self.post.as_deref()
    }
    fn resolve_category(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]> {
        self.category.as_deref()
    }
    fn resolve_page(&self) -> Option<&[rblog_content::theme::TemplateDescriptor]> {
        self.page.as_deref()
    }
}

/// Slim alternative shape for hand-written themes that don't want the full
/// Halo wrapper. The fields are exactly the same as `ThemeManifest` but
/// without `apiVersion` / `kind` / `metadata`.
#[derive(Debug, serde::Deserialize)]
struct SlimManifest {
    name: String,
    spec: ThemeSpec,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_slim_manifest() {
        let yaml = r"
name: default
spec:
  displayName: rblog default
  author:
    name: rblog
  version: 0.1.0
";
        let m = ThemeManifest::from_yaml_str(yaml, "test").unwrap();
        assert_eq!(m.name, "default");
        assert_eq!(m.spec.display_name, "rblog default");
    }

    #[test]
    fn parses_halo_wrapper_manifest() {
        let yaml = r"
apiVersion: theme.halo.run/v1alpha1
kind: Theme
metadata:
  name: anatole
spec:
  displayName: Anatole
  author:
    name: hi-caicai
  version: 1.0.0
  requires: '*'
  customTemplates:
    post:
      - name: featured
        description: Featured post layout
        file: post-featured.html
";
        let m = ThemeManifest::from_yaml_str(yaml, "test").unwrap();
        assert_eq!(m.name, "anatole");
        assert_eq!(m.spec.display_name, "Anatole");
        assert_eq!(m.post_template(Some("featured")), "post-featured.html");
        assert_eq!(m.post_template(None), "post.html");
    }

    #[test]
    fn missing_display_name_is_error() {
        let yaml = r#"name: foo
spec:
  displayName: ""
  author: { name: x }
"#;
        let err = ThemeManifest::from_yaml_str(yaml, "test").expect_err("must fail");
        assert!(matches!(err, ThemeManifestError::MissingDisplayName { .. }));
    }
}
