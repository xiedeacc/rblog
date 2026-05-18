//! Theme discovery + registry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use rblog_content::render::MarkdownPipeline;
use thiserror::Error;
use tracing::info;
use walkdir::WalkDir;

use crate::manifest::{ThemeManifest, ThemeManifestError};
use crate::renderer::ThemeRenderer;

#[derive(Debug, Error)]
pub enum ThemeRegistryError {
    #[error("theme root `{0}` does not exist")]
    RootMissing(PathBuf),
    #[error("active theme `{0}` is not loaded")]
    ActiveNotLoaded(String),
    #[error("manifest error: {0}")]
    Manifest(#[from] ThemeManifestError),
}

/// A loaded theme. Cloning is cheap (Arcs all the way down).
#[derive(Clone)]
pub struct LoadedTheme {
    pub manifest: Arc<ThemeManifest>,
    pub renderer: Arc<ThemeRenderer>,
    pub dir: PathBuf,
}

/// Discovers every `<root>/<theme>/theme.yaml` and builds a [`LoadedTheme`]
/// for it. Tracks which one is "active" — the HTTP layer renders public
/// pages using that theme.
pub struct ThemeRegistry {
    root: PathBuf,
    pipeline: Arc<MarkdownPipeline>,
    inner: RwLock<Inner>,
}

#[derive(Default)]
struct Inner {
    themes: HashMap<String, LoadedTheme>,
    active: String,
}

impl ThemeRegistry {
    /// Construct an empty registry rooted at `root`.
    pub fn new(root: PathBuf, pipeline: Arc<MarkdownPipeline>) -> Self {
        Self {
            root,
            pipeline,
            inner: RwLock::new(Inner::default()),
        }
    }

    /// Discover and load every theme below `root`. Themes without a
    /// parseable `theme.yaml` are logged and skipped.
    pub fn reload(&self) -> Result<(), ThemeRegistryError> {
        if !self.root.exists() {
            return Err(ThemeRegistryError::RootMissing(self.root.clone()));
        }
        let mut new_themes = HashMap::new();
        for entry in WalkDir::new(&self.root).min_depth(2).max_depth(2) {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.file_name() != "theme.yaml" {
                continue;
            }
            let theme_dir = entry.path().parent().unwrap_or(Path::new(".")).to_owned();
            match ThemeManifest::from_yaml_file(entry.path()) {
                Ok(manifest) => {
                    let renderer = ThemeRenderer::with_shared_markdown(
                        manifest.name.clone(),
                        theme_dir.clone(),
                        Arc::clone(&self.pipeline),
                    );
                    info!(theme = %manifest.name, dir = %theme_dir.display(), "loaded theme");
                    new_themes.insert(
                        manifest.name.clone(),
                        LoadedTheme {
                            manifest: Arc::new(manifest),
                            renderer: Arc::new(renderer),
                            dir: theme_dir,
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, path = %entry.path().display(), "skip theme");
                }
            }
        }
        let mut inner = self.inner.write();
        inner.themes = new_themes;
        if inner.active.is_empty() {
            if inner.themes.contains_key("default") {
                "default".clone_into(&mut inner.active);
            } else if let Some(first) = inner.themes.keys().next().cloned() {
                inner.active = first;
            }
        }
        Ok(())
    }

    /// Number of themes loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().themes.len()
    }

    /// Whether no themes are loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Find a loaded theme by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<LoadedTheme> {
        self.inner.read().themes.get(name).cloned()
    }

    /// The currently-active theme, used by the public SSR layer.
    pub fn active(&self) -> Result<LoadedTheme, ThemeRegistryError> {
        let inner = self.inner.read();
        let name = inner.active.clone();
        inner
            .themes
            .get(&name)
            .cloned()
            .ok_or(ThemeRegistryError::ActiveNotLoaded(name))
    }

    /// Set the active theme name. Errors if no theme with that name is
    /// currently loaded.
    pub fn set_active(&self, name: &str) -> Result<(), ThemeRegistryError> {
        let mut inner = self.inner.write();
        if !inner.themes.contains_key(name) {
            return Err(ThemeRegistryError::ActiveNotLoaded(name.to_owned()));
        }
        name.clone_into(&mut inner.active);
        Ok(())
    }

    /// List names of loaded themes (unordered).
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.inner.read().themes.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    fn make_theme(dir: &Path, name: &str, display: &str) {
        let theme_dir = dir.join(name);
        fs::create_dir_all(theme_dir.join("templates")).unwrap();
        fs::write(
            theme_dir.join("theme.yaml"),
            format!(
                "name: {name}\nspec:\n  displayName: {display}\n  author:\n    name: rblog\n  version: 0.1.0\n",
            ),
        )
        .unwrap();
        fs::write(
            theme_dir.join("templates/index.html"),
            "<html>{{ title }}</html>",
        )
        .unwrap();
    }

    #[test]
    fn reload_picks_up_themes() {
        let tmp = tempfile::tempdir().unwrap();
        make_theme(tmp.path(), "default", "rblog default");
        make_theme(tmp.path(), "anatole", "Anatole");
        let reg = ThemeRegistry::new(tmp.path().to_owned(), Arc::new(MarkdownPipeline::new()));
        reg.reload().unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.get("default").is_some());
        assert_eq!(reg.active().unwrap().manifest.name, "default");
    }

    #[test]
    fn set_active_validates() {
        let tmp = tempfile::tempdir().unwrap();
        make_theme(tmp.path(), "default", "rblog default");
        let reg = ThemeRegistry::new(tmp.path().to_owned(), Arc::new(MarkdownPipeline::new()));
        reg.reload().unwrap();
        let err = reg.set_active("nope").expect_err("must fail");
        assert!(matches!(err, ThemeRegistryError::ActiveNotLoaded(_)));
        reg.set_active("default").unwrap();
        assert_eq!(reg.active().unwrap().manifest.name, "default");
    }

    #[test]
    fn missing_root_errors() {
        let reg = ThemeRegistry::new(
            PathBuf::from("/no/such/path/rblog"),
            Arc::new(MarkdownPipeline::new()),
        );
        let err = reg.reload().expect_err("must fail");
        assert!(matches!(err, ThemeRegistryError::RootMissing(_)));
    }
}
