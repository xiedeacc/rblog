//! MiniJinja environment wrapper.

use std::path::PathBuf;
use std::sync::Arc;

use minijinja::{path_loader, Environment};
use rblog_content::render::MarkdownPipeline;
use thiserror::Error;

use crate::filters;

#[derive(Debug, Error)]
pub enum ThemeRendererError {
    #[error("template `{template}` not found in theme `{theme}`")]
    TemplateMissing { template: String, theme: String },
    #[error("template render failed: {0}")]
    Render(#[from] minijinja::Error),
}

/// Loads templates from `<theme_dir>/templates/` and wires up rblog filters.
///
/// Re-creating a [`ThemeRenderer`] is cheap; the heavy state lives on
/// [`Environment`]. Themes that change at runtime require a re-instantiate
/// (the HTTP layer wraps this in `parking_lot::RwLock` for hot-swap).
pub struct ThemeRenderer {
    theme_name: String,
    env: Environment<'static>,
}

impl ThemeRenderer {
    /// Build a renderer rooted at `theme_dir`. The directory must contain a
    /// `templates/` subfolder; missing templates fail at render-time, not
    /// build-time, mirroring MiniJinja's default loader behaviour.
    pub fn new(theme_name: impl Into<String>, theme_dir: PathBuf) -> Self {
        let mut env = Environment::new();
        let templates_dir = theme_dir.join("templates");
        env.set_loader(path_loader(templates_dir));
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
        env.set_fuel(Some(1_000_000));
        filters::register_all(&mut env);
        Self {
            theme_name: theme_name.into(),
            env,
        }
    }

    /// Same as [`Self::new`] but with a shared markdown pipeline injected so
    /// every render of the `| markdown` filter shares syntax-highlighting
    /// state.
    pub fn with_shared_markdown(
        theme_name: impl Into<String>,
        theme_dir: PathBuf,
        pipeline: Arc<MarkdownPipeline>,
    ) -> Self {
        let mut renderer = Self::new(theme_name, theme_dir);
        filters::register_markdown(&mut renderer.env, pipeline);
        renderer
    }

    /// Render `template` (relative path inside `templates/`) with the given
    /// context. Context is anything that serializes to a `serde_json::Value`.
    pub fn render<S: serde::Serialize>(
        &self,
        template: &str,
        ctx: &S,
    ) -> Result<String, ThemeRendererError> {
        let tmpl = self.env.get_template(template).map_err(|e| {
            if matches!(e.kind(), minijinja::ErrorKind::TemplateNotFound) {
                ThemeRendererError::TemplateMissing {
                    template: template.to_owned(),
                    theme: self.theme_name.clone(),
                }
            } else {
                ThemeRendererError::Render(e)
            }
        })?;
        Ok(tmpl.render(ctx)?)
    }

    #[must_use]
    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;

    fn build_test_theme(dir: &std::path::Path) {
        let templates = dir.join("templates");
        fs::create_dir_all(&templates).unwrap();
        fs::write(
            templates.join("base.html"),
            "<html><body>{% block body %}{% endblock %}</body></html>",
        )
        .unwrap();
        fs::write(
            templates.join("index.html"),
            "{% extends 'base.html' %}{% block body %}<h1>{{ title }}</h1>{% for p in posts %}<p>{{ p }}</p>{% endfor %}{% endblock %}",
        )
        .unwrap();
    }

    #[test]
    fn renders_with_inheritance() {
        let dir = tempfile::tempdir().unwrap();
        build_test_theme(dir.path());
        let r = ThemeRenderer::new("test", dir.path().to_owned());
        let ctx = serde_json::json!({
            "title": "Hello",
            "posts": ["a", "b"],
        });
        let out = r.render("index.html", &ctx).unwrap();
        assert!(out.contains("<h1>Hello</h1>"));
        assert!(out.contains("<p>a</p>"));
        assert!(out.contains("<p>b</p>"));
    }

    #[test]
    fn missing_template_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        build_test_theme(dir.path());
        let r = ThemeRenderer::new("test", dir.path().to_owned());
        let err = r
            .render("missing.html", &serde_json::json!({}))
            .expect_err("missing");
        assert!(matches!(
            err,
            ThemeRendererError::TemplateMissing { ref template, ref theme }
                if template == "missing.html" && theme == "test"
        ));
    }

    #[test]
    fn markdown_filter_works_in_rendered_template() {
        let dir = tempfile::tempdir().unwrap();
        let templates = dir.path().join("templates");
        fs::create_dir_all(&templates).unwrap();
        fs::write(templates.join("post.html"), "{{ raw | markdown }}").unwrap();
        let r = ThemeRenderer::new("test", dir.path().to_owned());
        let out = r
            .render("post.html", &serde_json::json!({"raw": "# Hi"}))
            .unwrap();
        assert!(out.contains("<h1"));
    }

    #[test]
    fn date_filter_in_template() {
        let dir = tempfile::tempdir().unwrap();
        let templates = dir.path().join("templates");
        fs::create_dir_all(&templates).unwrap();
        fs::write(templates.join("p.html"), "{{ t | date('%Y') }}").unwrap();
        let r = ThemeRenderer::new("test", dir.path().to_owned());
        let out = r
            .render("p.html", &serde_json::json!({"t": "2026-05-16T00:00:00Z"}))
            .unwrap();
        assert_eq!(out, "2026");
    }
}
