//! Permalink builders for rblog's public URL patterns:
//!
//! - `/archives/{slug}` for posts
//! - `/{slug}` for single pages
//! - `/tags/{slug}` for tag archives
//! - `/categories/{slug}` for category archives
//!
//! The HTTP layer combines these with the canonical base URL stored in the
//! `system` ConfigMap. Services emit the path component only — relative URLs
//! are always safe to embed in SSR templates.

pub(crate) fn post(slug: &str) -> String {
    format!("/archives/{}", slug.trim_start_matches('/'))
}

#[allow(dead_code)]
pub(crate) fn page(slug: &str) -> String {
    format!("/{}", slug.trim_start_matches('/'))
}

pub(crate) fn tag(slug: &str) -> String {
    format!("/tags/{}", slug.trim_start_matches('/'))
}

pub(crate) fn category(slug: &str) -> String {
    format!("/categories/{}", slug.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn permalink_shapes() {
        assert_eq!(post("hello"), "/archives/hello");
        assert_eq!(post("/hello"), "/archives/hello");
        assert_eq!(page("about"), "/about");
        assert_eq!(tag("rust"), "/tags/rust");
        assert_eq!(category("news"), "/categories/news");
    }
}
