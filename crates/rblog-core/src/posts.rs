//! Post + Snapshot management.
//!
//! Every post owns at least one base snapshot. The post's `spec.releaseSnapshot`
//! / `spec.headSnapshot` / `spec.baseSnapshot` are kept in sync by this
//! service so callers don't need to understand Halo's snapshot chain.

use std::sync::Arc;

use chrono::{Duration, Utc};
use rblog_content::content::{Excerpt, Post, PostSpec, Snapshot, SnapshotSpec, Visible};
use rblog_content::content_wrapper::{compose_snapshot, KEEP_RAW_ANNOTATION};
use rblog_content::infra::Ref;
use rblog_content::render::{MarkdownPipeline, RenderOptions};
use rblog_index::{
    FieldSelector, IndexEngine, IndexedExt, LabelSelector, ListOptions, ListResult, SortDirection,
};

fn array_field_contains(entry: &IndexedExt, path: &str, value: &str) -> bool {
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
use rblog_scheme::{Extension, Metadata};
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;
use uuid::Uuid;

use crate::indexing::{remove, upsert};
use crate::permalink;
use crate::{conflict, not_found, ServiceError};

/// Label key marking a post as published.
pub const PUBLISHED_LABEL: &str = "content.halo.run/published";

/// Label key marking a post as soft-deleted.
pub const DELETED_LABEL: &str = "content.halo.run/deleted";

/// Annotation pointing at the post's last released snapshot.
pub const LAST_RELEASED_ANNO: &str = "content.halo.run/last-released-snapshot";

#[derive(Clone)]
pub struct PostService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
    pipeline: Arc<MarkdownPipeline>,
}

impl PostService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>, pipeline: Arc<MarkdownPipeline>) -> Self {
        Self {
            pool,
            index,
            pipeline,
        }
    }

    /// Create a draft post. Generates a base snapshot from `draft.markdown`
    /// and ties it to the new post's `spec.{base,head}Snapshot` pointers.
    pub async fn draft(&self, draft: DraftPost) -> Result<PostDetail, ServiceError> {
        if draft.title.trim().is_empty() {
            return Err(ServiceError::Validation("title must not be empty".into()));
        }
        if draft.slug.trim().is_empty() {
            return Err(ServiceError::Validation("slug must not be empty".into()));
        }
        let store = TypedStore::new(&self.pool);
        if store.fetch::<Post>(&draft.name).await?.is_some() {
            return Err(conflict("Post", draft.name));
        }
        let rendered = self
            .pipeline
            .render(&draft.markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let snapshot_name = Uuid::new_v4().to_string();
        let mut snapshot = Snapshot::new(&snapshot_name).with_spec(SnapshotSpec {
            subject_ref: Ref::of_gvk(&draft.name, &Post::gvk()),
            raw_type: "markdown".to_owned(),
            raw_patch: Some(draft.markdown.clone()),
            content_patch: Some(rendered.html.clone()),
            parent_snapshot_name: None,
            last_modify_time: Some(Utc::now()),
            owner: draft.owner.clone(),
            contributors: None,
        });
        snapshot
            .metadata
            .set_annotation(KEEP_RAW_ANNOTATION, "true");
        let saved_snapshot = store.create(&snapshot).await?;
        upsert(&self.index, &saved_snapshot)?;

        let mut post = Post::new(&draft.name).with_spec(PostSpec {
            title: draft.title.clone(),
            slug: draft.slug.clone(),
            release_snapshot: None,
            head_snapshot: Some(snapshot_name.clone()),
            base_snapshot: Some(snapshot_name),
            owner: Some(draft.owner.clone()),
            template: draft.template.clone(),
            cover: draft.cover.clone(),
            deleted: false,
            publish: false,
            publish_time: None,
            pinned: draft.pinned.unwrap_or(false),
            allow_comment: draft.allow_comment.unwrap_or(true),
            visible: draft.visible,
            priority: draft.priority.unwrap_or(0),
            excerpt: Excerpt {
                auto_generate: draft
                    .excerpt
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty()),
                raw: draft.excerpt.filter(|value| !value.trim().is_empty()),
            },
            categories: draft.categories.clone(),
            tags: draft.tags.clone(),
            html_metas: None,
        });
        post.metadata.set_label(PUBLISHED_LABEL, "false");
        post.metadata.set_label(DELETED_LABEL, "false");
        let saved_post = store.create(&post).await?;
        upsert(&self.index, &saved_post)?;
        self.build_detail(
            saved_post,
            &rendered.html,
            &rendered.excerpt,
            &draft.markdown,
        )
    }

    /// Replace post content. Creates a new snapshot diffing against the
    /// existing base, or updates the base in place if `replace_base` is set
    /// (admin "edit base" path).
    pub async fn update_content(
        &self,
        name: &str,
        markdown: &str,
        author: &str,
    ) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        let base_name = post
            .spec
            .as_ref()
            .and_then(|s| s.base_snapshot.clone())
            .ok_or_else(|| {
                ServiceError::Internal(format!("post `{name}` missing base snapshot"))
            })?;
        let mut base: Snapshot = store
            .fetch::<Snapshot>(&base_name)
            .await?
            .ok_or_else(|| not_found("Snapshot", &base_name))?;

        let rendered = self
            .pipeline
            .render(markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;

        // Simplest model for v1: replace the base snapshot body in place.
        // The history is reconstructable later via a separate `Snapshot`
        // history pipe (out of scope for this commit).
        if let Some(spec) = base.spec.as_mut() {
            spec.raw_patch = Some(markdown.to_owned());
            spec.content_patch = Some(rendered.html.clone());
            spec.last_modify_time = Some(Utc::now());
            let mut contributors = spec.contributors.clone().unwrap_or_default();
            contributors.insert(author.to_owned());
            spec.contributors = Some(contributors);
        }
        let saved_base = store.update(&base).await?;
        upsert(&self.index, &saved_base)?;

        if let Some(spec) = post.spec.as_mut() {
            spec.head_snapshot = Some(base_name);
        }
        let saved_post = store.update(&post).await?;
        upsert(&self.index, &saved_post)?;
        self.build_detail(saved_post, &rendered.html, &rendered.excerpt, markdown)
    }

    pub async fn update_settings(
        &self,
        name: &str,
        settings: PostSettingsUpdate,
    ) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        if let Some(spec) = post.spec.as_mut() {
            if let Some(title) = settings.title {
                if title.trim().is_empty() {
                    return Err(ServiceError::Validation("title must not be empty".into()));
                }
                spec.title = title;
            }
            if let Some(slug) = settings.slug {
                if slug.trim().is_empty() {
                    return Err(ServiceError::Validation("slug must not be empty".into()));
                }
                spec.slug = slug;
            }
            if let Some(excerpt) = settings.excerpt {
                spec.excerpt.raw = if excerpt.trim().is_empty() {
                    None
                } else {
                    Some(excerpt)
                };
                spec.excerpt.auto_generate = spec.excerpt.raw.is_none();
            }
            if let Some(visible) = settings.visible {
                spec.visible = visible;
            }
            if let Some(cover) = settings.cover {
                spec.cover = if cover.trim().is_empty() {
                    None
                } else {
                    Some(cover)
                };
            }
            if let Some(template) = settings.template {
                spec.template = if template.trim().is_empty() {
                    None
                } else {
                    Some(template)
                };
            }
            if let Some(priority) = settings.priority {
                spec.priority = priority;
            }
            if let Some(pinned) = settings.pinned {
                spec.pinned = pinned;
            }
            if let Some(allow_comment) = settings.allow_comment {
                spec.allow_comment = allow_comment;
            }
            if let Some(publish_time) = settings.publish_time {
                spec.publish_time = publish_time;
            }
        }
        let saved = store.update(&post).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    /// Mark a post as published. Sets the `release_snapshot` pointer to the
    /// current head snapshot, flips `spec.publish`, sets `publish_time` to
    /// now, and writes the published label.
    pub async fn publish(
        &self,
        name: &str,
        opts: PublishOptions,
    ) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        let head = post
            .spec
            .as_ref()
            .and_then(|s| s.head_snapshot.clone())
            .ok_or_else(|| {
                ServiceError::Internal(format!("post `{name}` missing head snapshot"))
            })?;
        if let Some(spec) = post.spec.as_mut() {
            spec.publish = true;
            spec.publish_time = Some(opts.publish_time.unwrap_or_else(Utc::now));
            spec.release_snapshot = Some(head.clone());
            if let Some(visible) = opts.visible {
                spec.visible = visible;
            }
        }
        post.metadata.set_label(PUBLISHED_LABEL, "true");
        post.metadata.set_annotation(LAST_RELEASED_ANNO, &head);
        let saved = store.update(&post).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    /// Unpublish: keep the post but flip `publish=false` and remove the
    /// published label. Snapshots are not touched.
    pub async fn unpublish(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        if let Some(spec) = post.spec.as_mut() {
            spec.publish = false;
        }
        post.metadata.set_label(PUBLISHED_LABEL, "false");
        let saved = store.update(&post).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    /// Soft-delete: sets `spec.deleted = true` and `metadata.deletionTimestamp`.
    /// The HTTP layer hides deleted posts from public routes.
    pub async fn soft_delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        if let Some(spec) = post.spec.as_mut() {
            spec.deleted = true;
        }
        post.metadata.set_label(DELETED_LABEL, "true");
        post.metadata.set_label(PUBLISHED_LABEL, "false");
        post.metadata.deletion_timestamp = Some(Utc::now());
        let saved = store.update(&post).await?;
        upsert(&self.index, &saved)?;
        Ok(())
    }

    /// Restore a post from the recycle bin.
    pub async fn restore(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        let published = post.spec.as_ref().is_some_and(|spec| spec.publish);
        if let Some(spec) = post.spec.as_mut() {
            spec.deleted = false;
        }
        post.metadata.deletion_timestamp = None;
        post.metadata.set_label(DELETED_LABEL, "false");
        post.metadata
            .set_label(PUBLISHED_LABEL, published.to_string());
        let saved = store.update(&post).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    /// Permanently remove posts that have stayed in the recycle bin for 180 days.
    pub async fn purge_expired_deleted(&self) -> Result<usize, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let cutoff = Utc::now() - Duration::days(180);
        let posts: Vec<Post> = store.list::<Post>().await?;
        let mut purged = 0;
        for post in posts {
            let deleted =
                post.spec.as_ref().is_some_and(|spec| spec.deleted) || post.metadata.is_deleted();
            let expired = post
                .metadata
                .deletion_timestamp
                .is_some_and(|deleted_at| deleted_at <= cutoff);
            if deleted && expired {
                self.purge(post.metadata.name()).await?;
                purged += 1;
            }
        }
        Ok(purged)
    }

    /// Hard delete: removes the post and every snapshot referencing it.
    pub async fn purge(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        // Best-effort: remove every snapshot belonging to this post.
        let snaps: Vec<Snapshot> = store.list::<Snapshot>().await?;
        for snap in snaps {
            if snap
                .spec
                .as_ref()
                .is_some_and(|s| s.subject_ref.kind == "Post" && s.subject_ref.name == name)
            {
                let _ = store.delete(&snap).await;
                remove::<Snapshot>(&self.index, snap.metadata.name());
            }
        }
        store.delete(&post).await?;
        remove::<Post>(&self.index, name);
        Ok(())
    }

    /// List posts matching a public query. Filters out soft-deleted ones.
    pub fn list(&self, query: PostListQuery) -> Result<PostListPage, ServiceError> {
        let mut opts = ListOptions::default().sorted_by("spec.publishTime", SortDirection::Desc);
        match query.status {
            PostStatusFilter::Published => {
                opts = opts.with_label(LabelSelector::Equals {
                    key: PUBLISHED_LABEL.to_owned(),
                    value: "true".to_owned(),
                });
            }
            PostStatusFilter::Draft => {
                opts = opts.with_label(LabelSelector::Equals {
                    key: PUBLISHED_LABEL.to_owned(),
                    value: "false".to_owned(),
                });
            }
            PostStatusFilter::Any => {}
        }
        if query.deleted_only {
            opts = opts.with_label(LabelSelector::Equals {
                key: DELETED_LABEL.to_owned(),
                value: "true".to_owned(),
            });
        } else if !query.include_deleted {
            opts = opts.with_label(LabelSelector::NotEquals {
                key: DELETED_LABEL.to_owned(),
                value: "true".to_owned(),
            });
        }
        if let Some(visible) = query.visible {
            opts = opts.with_field(FieldSelector::Equals {
                path: "spec.visible".to_owned(),
                value: serde_json::to_value(visible)
                    .map_err(|e| ServiceError::Internal(format!("encode visibility: {e}")))?,
            });
        }
        if query.public_only {
            opts = opts.with_field(FieldSelector::Equals {
                path: "spec.visible".to_owned(),
                value: serde_json::Value::String("PUBLIC".to_owned()),
            });
        }
        // Tag / category are 1:N arrays — handle them as a post-filter so
        // we can use the simple JSON-equality index for everything else.
        let tag_filter = query.tag.clone();
        let category_filter = query.category.clone();
        let needs_array_filter = tag_filter.is_some() || category_filter.is_some();
        if !needs_array_filter {
            opts = opts.paged(query.offset, query.limit);
        }
        let ListResult { items, mut total } = self.index.list(&Post::gvk(), &opts)?;
        let mut items = items;
        if needs_array_filter {
            items.retain(|entry| {
                tag_filter
                    .as_deref()
                    .is_none_or(|t| array_field_contains(entry, "spec.tags", t))
                    && category_filter
                        .as_deref()
                        .is_none_or(|c| array_field_contains(entry, "spec.categories", c))
            });
            total = items.len();
            items = items
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect();
        }
        let mut list_items = Vec::with_capacity(items.len());
        for entry in items {
            let post: Post = serde_json::from_value(entry.raw)
                .map_err(|e| ServiceError::Internal(format!("decode Post: {e}")))?;
            list_items.push(PostListItem::from_post(&post));
        }
        Ok(PostListPage {
            items: list_items,
            total,
        })
    }

    /// Get the full detail (composed HTML + metadata) for a published post
    /// by name. Returns `NotFound` if missing or unpublished.
    pub async fn public_detail(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let detail = self.detail_from_store(name).await?;
        if !detail.published || detail.deleted || detail.visible != Visible::Public {
            return Err(not_found("Post", name));
        }
        Ok(detail)
    }

    /// Get the full detail (regardless of published state). Admin-only.
    pub async fn admin_detail(&self, name: &str) -> Result<PostDetail, ServiceError> {
        self.detail_from_store(name).await
    }

    /// Public lookup by slug. Walks the index for an exact `spec.slug` match,
    /// then loads the post. Returns `None` on miss so the HTTP layer can map
    /// to 404.
    pub async fn public_by_slug(&self, slug: &str) -> Result<PostDetail, ServiceError> {
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
            })
            .with_field(FieldSelector::Equals {
                path: "spec.slug".to_owned(),
                value: serde_json::Value::String(slug.to_owned()),
            })
            .paged(0, 1);
        let res = self.index.list(&Post::gvk(), &opts)?;
        let entry = res
            .items
            .into_iter()
            .next()
            .ok_or_else(|| not_found("Post", slug))?;
        self.detail_from_store(&entry.name).await
    }

    async fn detail_from_store(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let post: Post = store
            .fetch::<Post>(name)
            .await?
            .ok_or_else(|| not_found("Post", name))?;
        let head = post
            .spec
            .as_ref()
            .and_then(|s| s.head_snapshot.clone())
            .or_else(|| post.spec.as_ref().and_then(|s| s.base_snapshot.clone()))
            .ok_or_else(|| ServiceError::Internal(format!("post `{name}` missing snapshot")))?;
        let base_name = post
            .spec
            .as_ref()
            .and_then(|s| s.base_snapshot.clone())
            .unwrap_or_else(|| head.clone());
        let base: Snapshot = store
            .fetch::<Snapshot>(&base_name)
            .await?
            .ok_or_else(|| not_found("Snapshot", &base_name))?;
        let head_snap: Snapshot = if head == base_name {
            base.clone()
        } else {
            store
                .fetch::<Snapshot>(&head)
                .await?
                .ok_or_else(|| not_found("Snapshot", &head))?
        };
        let wrap = compose_snapshot(&head_snap, &base)
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let rendered = self
            .pipeline
            .render(&wrap.raw, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        self.build_detail(post, &rendered.html, &rendered.excerpt, &wrap.raw)
    }

    #[allow(clippy::unused_self)]
    fn build_detail(
        &self,
        post: Post,
        rendered_html: &str,
        excerpt: &str,
        raw_markdown: &str,
    ) -> Result<PostDetail, ServiceError> {
        let status_excerpt = post
            .status
            .as_ref()
            .and_then(|s| non_empty(s.excerpt.as_deref()));
        let spec = post
            .spec
            .clone()
            .ok_or_else(|| ServiceError::Internal("Post missing spec".to_owned()))?;
        let excerpt = non_empty(spec.excerpt.raw.as_deref())
            .or(status_excerpt)
            .unwrap_or(excerpt)
            .to_owned();
        let permalink = permalink::post(&spec.slug);
        let published = post
            .metadata
            .label(PUBLISHED_LABEL)
            .is_some_and(|v| v == "true");
        let deleted = post
            .metadata
            .label(DELETED_LABEL)
            .is_some_and(|v| v == "true")
            || post.metadata.is_deleted();
        let visits = post
            .metadata
            .annotation("content.halo.run/stats")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|stats| stats.get("visit").and_then(serde_json::Value::as_i64))
            .and_then(|visit| i32::try_from(visit).ok())
            .unwrap_or_default();
        Ok(PostDetail {
            name: post.metadata.name().to_owned(),
            title: spec.title,
            slug: spec.slug,
            permalink,
            content_html: rendered_html.to_owned(),
            raw_markdown: raw_markdown.to_owned(),
            excerpt,
            publish_time: spec.publish_time,
            published,
            deleted,
            visible: spec.visible,
            owner: spec.owner,
            categories: spec.categories.unwrap_or_default(),
            tags: spec.tags.unwrap_or_default(),
            cover: spec.cover,
            template: spec.template,
            pinned: spec.pinned,
            allow_comment: spec.allow_comment,
            priority: spec.priority,
            visits,
            metadata: post.metadata,
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DraftPost {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub markdown: String,
    pub owner: String,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub allow_comment: Option<bool>,
    #[serde(default)]
    pub visible: Visible,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PublishOptions {
    pub publish_time: Option<chrono::DateTime<Utc>>,
    pub visible: Option<Visible>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PostSettingsUpdate {
    pub title: Option<String>,
    pub slug: Option<String>,
    pub excerpt: Option<String>,
    pub visible: Option<Visible>,
    pub cover: Option<String>,
    pub template: Option<String>,
    pub priority: Option<i32>,
    pub pinned: Option<bool>,
    pub allow_comment: Option<bool>,
    pub publish_time: Option<Option<chrono::DateTime<Utc>>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PostListQuery {
    #[serde(default)]
    pub status: PostStatusFilter,
    #[serde(default)]
    pub include_deleted: bool,
    #[serde(default)]
    pub deleted_only: bool,
    #[serde(default)]
    pub visible: Option<Visible>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub public_only: bool,
}

impl Default for PostListQuery {
    fn default() -> Self {
        Self {
            status: PostStatusFilter::default(),
            include_deleted: false,
            deleted_only: false,
            visible: None,
            tag: None,
            category: None,
            offset: default_offset(),
            limit: default_limit(),
            public_only: false,
        }
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn default_offset() -> usize {
    0
}
fn default_limit() -> usize {
    10
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PostStatusFilter {
    #[default]
    Published,
    Draft,
    Any,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostListItem {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub publish_time: Option<chrono::DateTime<Utc>>,
    pub excerpt: Option<String>,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub cover: Option<String>,
    pub published: bool,
    pub visible: Visible,
    pub deleted: bool,
    pub deletion_time: Option<chrono::DateTime<Utc>>,
    pub creation_time: Option<chrono::DateTime<Utc>>,
    pub last_modify_time: Option<chrono::DateTime<Utc>>,
    pub comments_count: i32,
    pub visits: i32,
    pub priority: i32,
}

impl PostListItem {
    fn from_post(post: &Post) -> Self {
        let spec = post.spec.clone().unwrap_or_default();
        let status = post.status.as_ref();
        let status_excerpt = status.and_then(|s| non_empty(s.excerpt.as_deref()));
        let deleted = post
            .metadata
            .label(DELETED_LABEL)
            .is_some_and(|v| v == "true")
            || post.metadata.is_deleted()
            || spec.deleted;
        let visits = post
            .metadata
            .annotation("content.halo.run/stats")
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|stats| stats.get("visit").and_then(serde_json::Value::as_i64))
            .and_then(|visit| i32::try_from(visit).ok())
            .unwrap_or_default();
        Self {
            name: post.metadata.name().to_owned(),
            title: spec.title,
            slug: spec.slug.clone(),
            permalink: permalink::post(&spec.slug),
            publish_time: spec.publish_time,
            excerpt: non_empty(spec.excerpt.raw.as_deref())
                .or(status_excerpt)
                .map(str::to_owned),
            tags: spec.tags.unwrap_or_default(),
            categories: spec.categories.unwrap_or_default(),
            cover: spec.cover,
            published: post
                .metadata
                .label(PUBLISHED_LABEL)
                .is_some_and(|v| v == "true"),
            visible: spec.visible,
            deleted,
            deletion_time: post.metadata.deletion_timestamp,
            creation_time: post.metadata.creation_timestamp,
            last_modify_time: status
                .and_then(|s| s.last_modify_time)
                .or(post.metadata.creation_timestamp),
            comments_count: status.and_then(|s| s.comments_count).unwrap_or_default(),
            visits,
            priority: spec.priority,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PostListPage {
    pub items: Vec<PostListItem>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostDetail {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub content_html: String,
    pub raw_markdown: String,
    pub excerpt: String,
    pub publish_time: Option<chrono::DateTime<Utc>>,
    pub published: bool,
    pub deleted: bool,
    pub visible: Visible,
    pub owner: Option<String>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub cover: Option<String>,
    pub template: Option<String>,
    pub pinned: bool,
    pub allow_comment: bool,
    pub priority: i32,
    pub visits: i32,
    #[serde(skip)]
    pub metadata: Metadata,
}
