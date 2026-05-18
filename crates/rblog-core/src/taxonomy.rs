//! Tag and Category services.
//!
//! Halo models tags and categories as kinds in their own right (no DB-level
//! join table); a post's `spec.tags[]` and `spec.categories[]` hold the
//! string `metadata.name` of each `Tag`/`Category`. Counting visible posts
//! per term is a derived field — we compute it lazily on request rather than
//! maintaining a Halo-style `status.postCount` because the in-memory index
//! makes that essentially free.

use std::sync::Arc;

use rblog_content::content::{
    Category, CategorySpec, CategoryStatus, Post, Tag, TagSpec, TagStatus,
};
use rblog_index::{FieldSelector, IndexEngine, IndexedExt, LabelSelector, ListOptions};
use rblog_scheme::Extension;
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;

use crate::indexing::{remove, upsert};
use crate::permalink;
use crate::posts::{DELETED_LABEL, PUBLISHED_LABEL};
use crate::{conflict, not_found, ServiceError};

#[derive(Clone)]
pub struct TagService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
}

impl TagService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create(&self, new: NewTag) -> Result<Tag, ServiceError> {
        validate_slug(&new.slug)?;
        let store = TypedStore::new(&self.pool);
        if store.fetch::<Tag>(&new.name).await?.is_some() {
            return Err(conflict("Tag", new.name));
        }
        let tag = Tag::new(&new.name).with_spec(TagSpec {
            display_name: new.display_name,
            slug: new.slug,
            description: new.description,
            color: new.color,
            cover: new.cover,
        });
        let saved = store.create(&tag).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn update(&self, tag: &Tag) -> Result<Tag, ServiceError> {
        if let Some(spec) = tag.spec.as_ref() {
            validate_slug(&spec.slug)?;
        }
        let store = TypedStore::new(&self.pool);
        let saved = store.update(tag).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let tag = store
            .fetch::<Tag>(name)
            .await?
            .ok_or_else(|| not_found("Tag", name))?;
        store.delete(&tag).await?;
        remove::<Tag>(&self.index, name);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Result<Tag, ServiceError> {
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<Tag>(name)
            .await?
            .ok_or_else(|| not_found("Tag", name))
    }

    /// Compose a stats view: the tag itself + how many published, non-deleted
    /// posts reference it. Used by sidebar/tag-cloud widgets.
    pub fn stats(&self) -> Result<Vec<TagStats>, ServiceError> {
        let tags = self.index.list(&Tag::gvk(), &ListOptions::default())?;
        let mut out = Vec::with_capacity(tags.items.len());
        for entry in tags.items {
            let count = self.count_posts(&entry.name)?;
            let tag: Tag = serde_json::from_value(entry.raw)
                .map_err(|e| ServiceError::Internal(format!("decode Tag: {e}")))?;
            let slug = tag
                .spec
                .as_ref()
                .map(|s| s.slug.clone())
                .unwrap_or_default();
            out.push(TagStats {
                name: tag.metadata.name().to_owned(),
                display_name: tag
                    .spec
                    .as_ref()
                    .map(|s| s.display_name.clone())
                    .unwrap_or_default(),
                slug: slug.clone(),
                permalink: permalink::tag(&slug),
                color: tag.spec.as_ref().and_then(|s| s.color.clone()),
                post_count: count,
            });
        }
        out.sort_by_key(|t| std::cmp::Reverse(t.post_count));
        Ok(out)
    }

    fn count_posts(&self, tag_name: &str) -> Result<usize, ServiceError> {
        count_posts_containing(&self.index, "spec.tags", tag_name)
    }
}

#[derive(Clone)]
pub struct CategoryService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
}

impl CategoryService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create(&self, new: NewCategory) -> Result<Category, ServiceError> {
        validate_slug(&new.slug)?;
        let store = TypedStore::new(&self.pool);
        if store.fetch::<Category>(&new.name).await?.is_some() {
            return Err(conflict("Category", new.name));
        }
        let category = Category::new(&new.name).with_spec(CategorySpec {
            display_name: new.display_name,
            slug: new.slug,
            description: new.description,
            cover: new.cover,
            template: new.template,
            post_template: new.post_template,
            priority: new.priority,
            children: new.children,
            prevent_parent_post_cascade_query: false,
            hide_from_list: false,
        });
        let saved = store.create(&category).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn update(&self, category: &Category) -> Result<Category, ServiceError> {
        if let Some(spec) = category.spec.as_ref() {
            validate_slug(&spec.slug)?;
        }
        let store = TypedStore::new(&self.pool);
        let saved = store.update(category).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let category = store
            .fetch::<Category>(name)
            .await?
            .ok_or_else(|| not_found("Category", name))?;
        store.delete(&category).await?;
        remove::<Category>(&self.index, name);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Result<Category, ServiceError> {
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<Category>(name)
            .await?
            .ok_or_else(|| not_found("Category", name))
    }

    pub fn stats(&self) -> Result<Vec<CategoryStats>, ServiceError> {
        let res = self.index.list(&Category::gvk(), &ListOptions::default())?;
        let mut out = Vec::with_capacity(res.items.len());
        for entry in res.items {
            let count = self.count_posts(&entry.name)?;
            let cat: Category = serde_json::from_value(entry.raw)
                .map_err(|e| ServiceError::Internal(format!("decode Category: {e}")))?;
            let slug = cat
                .spec
                .as_ref()
                .map(|s| s.slug.clone())
                .unwrap_or_default();
            let priority = cat.spec.as_ref().map_or(0, |s| s.priority);
            out.push(CategoryStats {
                name: cat.metadata.name().to_owned(),
                display_name: cat
                    .spec
                    .as_ref()
                    .map(|s| s.display_name.clone())
                    .unwrap_or_default(),
                slug: slug.clone(),
                permalink: permalink::category(&slug),
                priority,
                post_count: count,
            });
        }
        out.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    fn count_posts(&self, name: &str) -> Result<usize, ServiceError> {
        count_posts_containing(&self.index, "spec.categories", name)
    }
}

/// Walk the live index and count posts whose `path` array contains `value`.
/// The index supports `==` on scalar fields, but post-tag and post-category
/// relationships are 1:N arrays — we'd need an inverted index per term to
/// answer this in O(1). For v1 we keep the index simple and pay an O(n)
/// scan; even with thousands of posts that's a few hundred microseconds in
/// release builds, and the call sites are background widgets.
fn count_posts_containing(
    index: &IndexEngine,
    path: &str,
    value: &str,
) -> Result<usize, ServiceError> {
    let opts = ListOptions::default()
        .with_label(LabelSelector::Equals {
            key: PUBLISHED_LABEL.to_owned(),
            value: "true".to_owned(),
        })
        .with_label(LabelSelector::NotEquals {
            key: DELETED_LABEL.to_owned(),
            value: "true".to_owned(),
        })
        .with_field(FieldSelector::Equals {
            path: "spec.visible".to_owned(),
            value: serde_json::Value::String("PUBLIC".to_owned()),
        });
    let res = index.list(&Post::gvk(), &opts)?;
    Ok(res
        .items
        .into_iter()
        .filter(|entry| spec_array_contains(entry, path, value))
        .count())
}

fn spec_array_contains(entry: &IndexedExt, path: &str, value: &str) -> bool {
    let mut cursor: &serde_json::Value = &entry.raw;
    for tok in path.split('.') {
        if tok.is_empty() {
            continue;
        }
        cursor = match cursor.get(tok) {
            Some(v) => v,
            None => return false,
        };
    }
    match cursor {
        serde_json::Value::Array(arr) => arr.iter().any(|v| v.as_str() == Some(value)),
        serde_json::Value::String(s) => s == value,
        _ => false,
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTag {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub post_template: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub children: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagStats {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub permalink: String,
    pub color: Option<String>,
    pub post_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryStats {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub permalink: String,
    pub priority: i32,
    pub post_count: usize,
}

fn validate_slug(slug: &str) -> Result<(), ServiceError> {
    if slug.is_empty() {
        return Err(ServiceError::Validation("slug must not be empty".into()));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ServiceError::Validation(format!(
            "slug `{slug}` must be ASCII alphanumeric / dash / underscore"
        )));
    }
    Ok(())
}

// Suppress unused-warnings on accessor types that the HTTP layer will use.
#[allow(dead_code)]
const _: &str = "rblog-core/taxonomy: stats types are part of the public API";

#[allow(dead_code)]
fn _ensure_status_types_used() {
    let _ = TagStatus::default();
    let _ = CategoryStatus::default();
}
