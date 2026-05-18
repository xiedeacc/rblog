//! Bridge between the domain layer and the tantivy index.
//!
//! The HTTP layer owns the "when to index" decision because keeping the
//! search index in lock-step with mutations would otherwise force
//! `rblog-core` to depend on `rblog-search`. We deliberately keep the
//! coupling at the edge of the system: every admin handler that creates,
//! updates, publishes, unpublishes, or deletes a post pipes the result
//! through one of the helpers below.

use rblog_content::content::Visible;
use rblog_core::{PostDetail, Services};
use rblog_search::{IndexPost, SearchError, SearchIndex};
use rblog_store::{AnyPool, TypedStore};
use tracing::warn;

/// Map a `PostDetail` to the index payload. Public so the binary's boot
/// path can call it during the initial reindex.
pub fn detail_to_index(detail: &PostDetail, body: &str) -> IndexPost {
    IndexPost {
        name: detail.name.clone(),
        title: detail.title.clone(),
        slug: detail.slug.clone(),
        permalink: detail.permalink.clone(),
        excerpt: detail.excerpt.clone(),
        body: body.to_owned(),
        tags: detail.tags.clone(),
        categories: detail.categories.clone(),
        publish_time: detail.publish_time,
    }
}

/// Convenience: index `detail` only if the post is currently published.
pub fn index_if_published(search: &SearchIndex, detail: &PostDetail) {
    if !detail.published || detail.deleted || detail.visible != Visible::Public {
        if let Err(e) = search.delete(&detail.name) {
            warn!(error = %e, name = %detail.name, "failed to remove post from search index");
        }
        return;
    }
    // tantivy doesn't need the rendered HTML; the composed markdown is in
    // `excerpt + content_html` already. We feed it the rendered text since
    // that's what end-users actually search for.
    let body = strip_html(&detail.content_html);
    let payload = detail_to_index(detail, &body);
    if let Err(e) = search.replace(&payload) {
        warn!(error = %e, name = %detail.name, "failed to update search index");
    }
}

/// Remove a post from the index by name.
pub fn delete(search: &SearchIndex, name: &str) {
    if let Err(e) = search.delete(name) {
        warn!(error = %e, %name, "failed to delete from search index");
    }
}

/// Rebuild the search index from the live store. Used at boot.
pub async fn rebuild_from_store(
    search: &SearchIndex,
    services: &Services,
    pool: &AnyPool,
) -> Result<usize, SearchError> {
    use rblog_content::content::Post;
    let store = TypedStore::new(pool);
    let posts: Vec<Post> = match store.list::<Post>().await {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "failed to load posts for search rebuild");
            return Ok(0);
        }
    };
    let mut payloads = Vec::new();
    for p in posts {
        let name = p.metadata.name.clone();
        if let Ok(detail) = services.posts.public_detail(&name).await {
            let body = strip_html(&detail.content_html);
            payloads.push(detail_to_index(&detail, &body));
        }
    }
    let count = payloads.len();
    search.rebuild(payloads)?;
    Ok(count)
}

fn strip_html(html: &str) -> String {
    // Cheap pass: replace block tags with spaces, drop everything else
    // between `<` and `>`. Good enough for full-text matching; we don't
    // need a real parser since the index runs on a tokenizer downstream.
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_drops_tags_keeps_text() {
        let s = strip_html("<p>Hello <strong>world</strong>!</p>");
        assert!(s.contains("Hello"));
        assert!(s.contains("world"));
        assert!(!s.contains('<'));
        assert!(!s.contains('>'));
    }
}
