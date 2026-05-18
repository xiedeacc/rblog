//! Markdown -> sanitized HTML pipeline.
//!
//! Pipeline order:
//! 1. Parse Markdown with [`comrak`] using GFM extensions (tables,
//!    strikethrough, task-lists, autolinks, footnotes, header IDs).
//! 2. Walk the parsed AST and replace `code_block` nodes that have a
//!    `info_string` with syntect-highlighted HTML (CSS classes, theme-able
//!    by the consumer).
//! 3. Render to HTML.
//! 4. Pass through [`ammonia`] to drop anything dangerous.
//!
//! The output also includes a plain-text excerpt and a reading-time estimate
//! that don't need a separate HTML re-parse.
//!
//! ## Why classes, not inline styles
//!
//! Inline styles bake the theme into the rendered HTML — when the user
//! changes themes (or the admin previews a different one), every post would
//! need a re-render. Class-based highlighting puts the colors in CSS and
//! the markup stays theme-agnostic.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{
    Arena, ExtensionOptions, Options, ParseOptions, RenderOptions as ComrakRenderOptions,
};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("syntax highlighting failed: {0}")]
    Highlight(String),
}

/// Knobs for [`MarkdownPipeline::render`].
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Maximum characters in the auto-generated excerpt (no truncation marker
    /// appended; the caller's responsibility).
    pub excerpt_chars: usize,
    /// Average reading speed in words per minute for `reading_time_minutes`.
    pub words_per_minute: usize,
    /// Class prefix for syntect-generated tokens, matching CSS conventions
    /// (e.g. `language-` for Prism compatibility, or `tok-` for custom).
    pub syntax_class_prefix: &'static str,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            excerpt_chars: 220,
            words_per_minute: 220,
            syntax_class_prefix: "tok-",
        }
    }
}

/// Output of the render pipeline.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub html: String,
    pub excerpt: String,
    pub word_count: usize,
    pub reading_time_minutes: usize,
}

/// Reusable pipeline with a pre-loaded [`SyntaxSet`]. Cheap to clone.
pub struct MarkdownPipeline {
    syntax_set: SyntaxSet,
    sanitizer: ammonia::Builder<'static>,
}

impl MarkdownPipeline {
    #[must_use]
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            sanitizer: default_sanitizer(),
        }
    }

    /// Render `markdown` to HTML using the given options.
    pub fn render(&self, markdown: &str, opts: &RenderOptions) -> Result<Rendered, RenderError> {
        let arena = Arena::new();
        let comrak_opts = comrak_options();
        let root = comrak::parse_document(&arena, markdown, &comrak_opts);

        // Replace code blocks with our syntect-rendered HTML.
        self.highlight_code_blocks(root, opts)?;

        let mut html = String::new();
        comrak::html::format_document(root, &comrak_opts, &mut html)
            .map_err(|e| RenderError::Highlight(format!("html write: {e}")))?;

        let html = self.sanitizer.clean(&html).to_string();

        let plain = extract_plaintext(root);
        let word_count = plain.split_whitespace().count();
        let reading_time_minutes = word_count.div_ceil(opts.words_per_minute);
        let excerpt = build_excerpt(&plain, opts.excerpt_chars);

        Ok(Rendered {
            html,
            excerpt,
            word_count,
            reading_time_minutes: reading_time_minutes.max(1),
        })
    }

    fn highlight_code_blocks<'a>(
        &self,
        node: &'a AstNode<'a>,
        opts: &RenderOptions,
    ) -> Result<(), RenderError> {
        let mut data = node.data.borrow_mut();
        if let NodeValue::CodeBlock(ref cb) = data.value {
            let lang = cb.info.split_ascii_whitespace().next().unwrap_or("").trim();
            if !lang.is_empty() {
                if let Some(syntax) = self.syntax_set.find_syntax_by_token(lang) {
                    let mut html = ClassedHTMLGenerator::new_with_class_style(
                        syntax,
                        &self.syntax_set,
                        ClassStyle::SpacedPrefixed {
                            prefix: opts.syntax_class_prefix,
                        },
                    );
                    for line in LinesWithEndings::from(&cb.literal) {
                        html.parse_html_for_line_which_includes_newline(line)
                            .map_err(|e| RenderError::Highlight(e.to_string()))?;
                    }
                    let highlighted = html.finalize();
                    let wrapped = format!(
                        "<pre class=\"code-block language-{lang}\"><code class=\"language-{lang}\">{highlighted}</code></pre>"
                    );
                    data.value = NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
                        block_type: 7,
                        literal: wrapped,
                    });
                }
            }
        }
        drop(data);
        for child in node.children() {
            self.highlight_code_blocks(child, opts)?;
        }
        Ok(())
    }
}

impl Default for MarkdownPipeline {
    fn default() -> Self {
        Self::new()
    }
}

fn comrak_options() -> Options<'static> {
    let extension = ExtensionOptions {
        strikethrough: true,
        tagfilter: true,
        table: true,
        autolink: true,
        tasklist: true,
        footnotes: true,
        description_lists: true,
        header_ids: Some("rblog-".to_owned()),
        ..ExtensionOptions::default()
    };

    let render = ComrakRenderOptions {
        // Pass HTML blocks (incl. our injected syntect output) through to the
        // sanitizer, which has the final say on what survives.
        unsafe_: true,
        hardbreaks: false,
        github_pre_lang: true,
        escape: false,
        ..ComrakRenderOptions::default()
    };

    Options {
        extension,
        parse: ParseOptions::default(),
        render,
    }
}

fn default_sanitizer() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_generic_attributes(["class", "id"])
        .add_tags([
            "details",
            "summary",
            "del",
            "mark",
            "section",
            "figure",
            "figcaption",
            "input",
        ])
        // Allow GFM task-list checkboxes (`<input type="checkbox" disabled />`).
        // `class` is already in the generic attribute set, so a
        // `task-list-item-checkbox` class survives untouched.
        .add_tag_attributes("input", ["type", "disabled", "checked"])
        .link_rel(Some("noopener noreferrer nofollow"))
        .url_relative(ammonia::UrlRelative::PassThrough);
    b
}

fn extract_plaintext<'a>(node: &'a AstNode<'a>) -> String {
    fn walk<'a>(n: &'a AstNode<'a>, out: &mut String) {
        let data = n.data.borrow();
        match &data.value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            NodeValue::CodeBlock(cb) => out.push_str(&cb.literal),
            _ => {}
        }
        drop(data);
        for child in n.children() {
            walk(child, out);
        }
        // Block-ish boundaries get a space.
        let data = n.data.borrow();
        if matches!(
            data.value,
            NodeValue::Paragraph
                | NodeValue::Heading(_)
                | NodeValue::Item(_)
                | NodeValue::BlockQuote
                | NodeValue::CodeBlock(_)
        ) {
            out.push(' ');
        }
    }
    let mut out = String::new();
    walk(node, &mut out);
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out.trim().to_owned()
}

fn build_excerpt(plain: &str, max_chars: usize) -> String {
    if plain.chars().count() <= max_chars {
        return plain.to_owned();
    }
    let mut acc = String::new();
    for ch in plain.chars().take(max_chars) {
        acc.push(ch);
    }
    if let Some(last_space) = acc.rfind(char::is_whitespace) {
        // Avoid mid-word truncation.
        acc.truncate(last_space);
    }
    acc.push('…');
    acc
}

/// Convenience helper for one-shot rendering with the default pipeline. For
/// hot paths, build a single [`MarkdownPipeline`] and call `render` directly.
pub fn render_markdown(markdown: &str) -> Result<Rendered, RenderError> {
    MarkdownPipeline::new().render(markdown, &RenderOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn renders_basic_markdown() {
        let r = render_markdown("# Hello\n\nThis is **bold**.").unwrap();
        assert!(r.html.contains("<h1"));
        assert!(r.html.contains("<strong>bold</strong>"));
        assert_eq!(r.word_count, 4);
        assert_eq!(r.reading_time_minutes, 1);
    }

    #[test]
    fn header_ids_get_prefixed() {
        let r = render_markdown("# Title").unwrap();
        assert!(r.html.contains("rblog-title"), "got: {}", r.html);
    }

    #[test]
    fn tables_strikethrough_and_tasklists_enabled() {
        let md = "| a | b |\n|---|---|\n| 1 | 2 |\n\n~~strike~~\n\n- [x] done\n- [ ] todo";
        let r = render_markdown(md).unwrap();
        assert!(r.html.contains("<table"));
        assert!(r.html.contains("<del>strike</del>"));
        assert!(r.html.contains("type=\"checkbox\""), "got: {}", r.html);
    }

    #[test]
    fn code_block_with_language_is_highlighted_with_classes() {
        let r = render_markdown("```rust\nfn main() {}\n```").unwrap();
        // Wrapping markup
        assert!(r.html.contains("language-rust"));
        // syntect token class
        assert!(
            r.html.contains("tok-"),
            "expected syntect class prefix, got: {}",
            r.html
        );
    }

    #[test]
    fn code_block_without_language_stays_unhighlighted() {
        let r = render_markdown("```\nplain text\n```").unwrap();
        // No syntect classes, but still a <pre><code> wrapper from comrak.
        assert!(r.html.contains("<pre"));
        assert!(!r.html.contains("tok-"));
    }

    #[test]
    fn raw_html_script_is_sanitized() {
        let r = render_markdown("hello <script>alert(1)</script>").unwrap();
        // Tag must be gone (escaping into &lt;script&gt; also counts as gone
        // because the browser will not execute it; but ammonia drops the
        // tag entirely).
        assert!(!r.html.contains("<script"));
        // And no live JavaScript URL anywhere.
        assert!(!r.html.contains("javascript:"));
    }

    #[test]
    fn javascript_url_in_link_is_dropped() {
        let r = render_markdown("[click](javascript:alert(1))").unwrap();
        assert!(!r.html.contains("javascript:"));
    }

    #[test]
    fn external_links_get_rel_attributes() {
        let r = render_markdown("[home](https://example.com)").unwrap();
        assert!(r.html.contains("rel=\"noopener noreferrer nofollow\""));
    }

    #[test]
    fn excerpt_trims_to_word_boundary() {
        let md = "lorem ipsum dolor sit amet consectetur adipiscing elit. \
                  sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.";
        let r = MarkdownPipeline::new()
            .render(
                md,
                &RenderOptions {
                    excerpt_chars: 30,
                    ..RenderOptions::default()
                },
            )
            .unwrap();
        assert!(r.excerpt.ends_with('…'));
        assert!(
            !r.excerpt.contains("incididunt"),
            "excerpt: {:?}",
            r.excerpt
        );
    }

    #[test]
    fn reading_time_rounds_up_at_least_one_minute() {
        let r = render_markdown("just a few words").unwrap();
        assert_eq!(r.reading_time_minutes, 1);
    }
}
