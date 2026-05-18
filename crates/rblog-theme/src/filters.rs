//! rblog-specific MiniJinja filters and globals.
//!
//! Registered on the [`Environment`] by [`crate::renderer::ThemeRenderer`].
//! Themes use them as plain Jinja filters / functions:
//!
//! ```jinja
//! {{ post.publish_time | date("%Y-%m-%d") }}
//! {{ post.content | safe }}
//! {{ tags | length | pluralize("tag", "tags") }}
//! {{ markdown(post.raw) }}
//! ```

use chrono::{DateTime, Utc};
use minijinja::value::{Kwargs, Rest, Value};
use minijinja::{Environment, Error, ErrorKind};
use rblog_content::render::{MarkdownPipeline, RenderOptions};
use std::sync::Arc;

/// Wire every rblog filter and global onto `env`.
pub fn register_all(env: &mut Environment<'_>) {
    env.add_filter("date", date_filter);
    env.add_filter("isoformat", isoformat_filter);
    env.add_filter("pluralize", pluralize_filter);
    env.add_filter("truncate_chars", truncate_chars_filter);
    env.add_filter("markdown", markdown_filter);
    env.add_function("now", now_function);
}

/// Add the markdown pipeline as a renderer-injected filter. The
/// `MarkdownPipeline` is cheap to construct but caches the syntect
/// `SyntaxSet`, so we share one across calls.
pub fn register_markdown(env: &mut Environment<'_>, pipeline: Arc<MarkdownPipeline>) {
    env.add_filter("markdown", move |body: &str| -> Result<Value, Error> {
        let r = pipeline
            .render(body, &RenderOptions::default())
            .map_err(|e| {
                Error::new(ErrorKind::InvalidOperation, format!("markdown render: {e}"))
            })?;
        Ok(Value::from_safe_string(r.html))
    });
}

fn date_filter(value: &Value, fmt: Option<&str>) -> Result<String, Error> {
    let s = parse_datetime(value)?;
    Ok(s.format(fmt.unwrap_or("%Y-%m-%d")).to_string())
}

fn isoformat_filter(value: &Value) -> Result<String, Error> {
    let s = parse_datetime(value)?;
    Ok(s.to_rfc3339())
}

fn parse_datetime(value: &Value) -> Result<DateTime<Utc>, Error> {
    if let Some(s) = value.as_str() {
        let d = DateTime::parse_from_rfc3339(s).map_err(|e| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("parse datetime `{s}`: {e}"),
            )
        })?;
        return Ok(d.with_timezone(&Utc));
    }
    Err(Error::new(
        ErrorKind::InvalidOperation,
        "expected an RFC3339 string for date filter",
    ))
}

fn pluralize_filter(count: i64, args: Rest<Value>, _kwargs: Kwargs) -> String {
    let singular = args
        .first()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let plural = args
        .get(1)
        .and_then(|v| v.as_str())
        .map_or_else(|| format!("{singular}s"), str::to_owned);
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn truncate_chars_filter(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    if let Some(last_space) = out.rfind(char::is_whitespace) {
        out.truncate(last_space);
    }
    out.push('…');
    out
}

fn markdown_filter(body: &str) -> Result<Value, Error> {
    let pipeline = MarkdownPipeline::new();
    let r = pipeline
        .render(body, &RenderOptions::default())
        .map_err(|e| Error::new(ErrorKind::InvalidOperation, format!("markdown render: {e}")))?;
    Ok(Value::from_safe_string(r.html))
}

fn now_function() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn env_with_filters() -> Environment<'static> {
        let mut env = Environment::new();
        register_all(&mut env);
        env
    }

    #[test]
    fn date_filter_formats_correctly() {
        let env = env_with_filters();
        let tmpl = env
            .template_from_str("{{ '2026-01-15T12:30:00Z' | date('%Y/%m/%d') }}")
            .unwrap();
        assert_eq!(tmpl.render(()).unwrap(), "2026/01/15");
    }

    #[test]
    fn pluralize_filter_picks_singular_or_plural() {
        let env = env_with_filters();
        let tmpl1 = env
            .template_from_str("{{ 1 | pluralize('post', 'posts') }}")
            .unwrap();
        assert_eq!(tmpl1.render(()).unwrap(), "post");
        let tmpl2 = env
            .template_from_str("{{ 5 | pluralize('post', 'posts') }}")
            .unwrap();
        assert_eq!(tmpl2.render(()).unwrap(), "posts");
        let tmpl3 = env
            .template_from_str("{{ 2 | pluralize('post') }}")
            .unwrap();
        assert_eq!(tmpl3.render(()).unwrap(), "posts");
    }

    #[test]
    fn truncate_chars_respects_word_boundary() {
        let env = env_with_filters();
        let tmpl = env
            .template_from_str("{{ 'hello world foo bar' | truncate_chars(11) }}")
            .unwrap();
        let out = tmpl.render(()).unwrap();
        assert!(out.ends_with('…'));
        assert!(!out.contains("foo"));
    }

    #[test]
    fn markdown_filter_returns_safe_html() {
        let env = env_with_filters();
        let tmpl = env.template_from_str("{{ '# Hello' | markdown }}").unwrap();
        let out = tmpl.render(()).unwrap();
        assert!(out.contains("<h1"));
    }
}
