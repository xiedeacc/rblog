//! SinglePage management for standalone pages such as `/about`.

use std::sync::Arc;

use chrono::Utc;
use rblog_content::content::{Comment, PostStatus, SinglePage, Snapshot, SnapshotSpec, Visible};
use rblog_content::content_wrapper::{compose_snapshot, KEEP_RAW_ANNOTATION};
use rblog_content::infra::Ref;
use rblog_content::render::{MarkdownPipeline, RenderOptions};
use rblog_index::{
    FieldSelector, IndexEngine, LabelSelector, ListOptions, ListResult, SortDirection,
};
use rblog_scheme::{Extension, Metadata};
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;
use uuid::Uuid;

use crate::indexing::upsert;
use crate::permalink;
use crate::{conflict, not_found, ServiceError};

const PUBLISHED_LABEL: &str = "content.halo.run/published";
const DELETED_LABEL: &str = "content.halo.run/deleted";
const LAST_RELEASED_ANNO: &str = "content.halo.run/last-released-snapshot";
const STATS_ANNO: &str = "content.halo.run/stats";
const APPROVED_LABEL: &str = "content.halo.run/approved";
const SUBJECT_KIND_LABEL: &str = "content.halo.run/subject-kind";
const SUBJECT_NAME_LABEL: &str = "content.halo.run/subject-name";

#[derive(Clone)]
pub struct PageService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
    pipeline: Arc<MarkdownPipeline>,
}

impl PageService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>, pipeline: Arc<MarkdownPipeline>) -> Self {
        Self {
            pool,
            index,
            pipeline,
        }
    }

    fn ensure_slug_available(
        &self,
        slug: &str,
        current_name: Option<&str>,
    ) -> Result<(), ServiceError> {
        let res = self.index.list(
            &SinglePage::gvk(),
            &ListOptions::default()
                .with_field(FieldSelector::Equals {
                    path: "spec.slug".to_owned(),
                    value: serde_json::Value::String(slug.to_owned()),
                })
                .paged(0, 2),
        )?;
        if res
            .items
            .iter()
            .any(|entry| current_name != Some(entry.name.as_str()))
        {
            return Err(conflict("SinglePage slug", slug));
        }
        Ok(())
    }

    pub async fn list(&self, query: PageListQuery) -> Result<PageListPage, ServiceError> {
        let mut opts = ListOptions::default().sorted_by("spec.publishTime", SortDirection::Desc);
        match query.status {
            PageStatusFilter::Published => {
                opts = opts.with_label(LabelSelector::Equals {
                    key: PUBLISHED_LABEL.to_owned(),
                    value: "true".to_owned(),
                });
            }
            PageStatusFilter::Draft => {
                opts = opts.with_label(LabelSelector::Equals {
                    key: PUBLISHED_LABEL.to_owned(),
                    value: "false".to_owned(),
                });
            }
            PageStatusFilter::Any => {}
        }
        if !query.include_deleted {
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
        let ListResult { items, .. } = self.index.list(&SinglePage::gvk(), &opts)?;
        let mut pages = Vec::with_capacity(items.len());
        for entry in items {
            let page: SinglePage = serde_json::from_value(entry.raw)
                .map_err(|e| ServiceError::Internal(format!("decode SinglePage: {e}")))?;
            let detail = self.detail_from_store(page.metadata.name()).await?;
            pages.push(PageListItem::from_detail(&detail));
        }
        pages.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| (!b.published).cmp(&(!a.published)))
                .then_with(|| page_list_time(b).cmp(&page_list_time(a)))
                .then_with(|| a.name.cmp(&b.name))
        });
        let total = pages.len();
        let items = pages
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();
        Ok(PageListPage { items, total })
    }

    pub async fn admin_detail(&self, name: &str) -> Result<PageDetail, ServiceError> {
        self.detail_from_store(name).await
    }

    pub async fn by_slug(
        &self,
        slug: &str,
        include_private: bool,
    ) -> Result<PageDetail, ServiceError> {
        let mut opts = ListOptions::default()
            .with_label(LabelSelector::Equals {
                key: PUBLISHED_LABEL.to_owned(),
                value: "true".to_owned(),
            })
            .with_label(LabelSelector::NotEquals {
                key: DELETED_LABEL.to_owned(),
                value: "true".to_owned(),
            });
        if !include_private {
            opts = opts.with_field(FieldSelector::Equals {
                path: "spec.visible".to_owned(),
                value: serde_json::Value::String("PUBLIC".to_owned()),
            });
        }
        let res = self.index.list(
            &SinglePage::gvk(),
            &opts
                .with_field(FieldSelector::Equals {
                    path: "spec.slug".to_owned(),
                    value: serde_json::Value::String(slug.to_owned()),
                })
                .paged(0, 1),
        )?;
        let entry = res
            .items
            .into_iter()
            .next()
            .ok_or_else(|| not_found("SinglePage", slug))?;
        self.detail_from_store(&entry.name).await
    }

    pub async fn update_content(
        &self,
        name: &str,
        markdown: &str,
        author: &str,
    ) -> Result<PageDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        let rendered = self
            .pipeline
            .render(markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let now = Utc::now();
        let base_name =
            if let Some(base_name) = page.spec.as_ref().and_then(|s| s.base_snapshot.clone()) {
                let mut base: Snapshot = store
                    .fetch::<Snapshot>(&base_name)
                    .await?
                    .ok_or_else(|| not_found("Snapshot", &base_name))?;
                if let Some(spec) = base.spec.as_mut() {
                    spec.raw_type = "markdown".to_owned();
                    spec.raw_patch = Some(markdown.to_owned());
                    spec.content_patch = Some(rendered.html.clone());
                    spec.last_modify_time = Some(now);
                    let mut contributors = spec.contributors.clone().unwrap_or_default();
                    contributors.insert(author.to_owned());
                    spec.contributors = Some(contributors);
                }
                let saved_base = store.update(&base).await?;
                upsert(&self.index, &saved_base)?;
                base_name
            } else {
                let snapshot_name = Uuid::new_v4().to_string();
                let mut snapshot = Snapshot::new(&snapshot_name).with_spec(SnapshotSpec {
                    subject_ref: Ref::of_gvk(name, &SinglePage::gvk()),
                    raw_type: "markdown".to_owned(),
                    raw_patch: Some(markdown.to_owned()),
                    content_patch: Some(rendered.html.clone()),
                    parent_snapshot_name: None,
                    last_modify_time: Some(now),
                    owner: author.to_owned(),
                    contributors: None,
                });
                snapshot
                    .metadata
                    .set_annotation(KEEP_RAW_ANNOTATION, "true");
                let saved_snapshot = store.create(&snapshot).await?;
                upsert(&self.index, &saved_snapshot)?;
                snapshot_name
            };
        if let Some(spec) = page.spec.as_mut() {
            spec.base_snapshot.get_or_insert_with(|| base_name.clone());
            spec.head_snapshot = Some(base_name);
        }
        page.status
            .get_or_insert_with(PostStatus::default)
            .last_modify_time = Some(now);
        let saved_page = store.update(&page).await?;
        upsert(&self.index, &saved_page)?;
        self.build_detail(
            saved_page,
            &rendered.html,
            &rendered.excerpt,
            markdown,
            "markdown",
        )
    }

    pub async fn update_settings(
        &self,
        name: &str,
        settings: PageSettingsUpdate,
    ) -> Result<PageDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        if let Some(spec) = page.spec.as_mut() {
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
                self.ensure_slug_available(&slug, Some(name))?;
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
        page.status
            .get_or_insert_with(PostStatus::default)
            .last_modify_time = Some(Utc::now());
        let saved = store.update(&page).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    pub async fn publish(&self, name: &str) -> Result<PageDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        let head = page
            .spec
            .as_ref()
            .and_then(|s| s.head_snapshot.clone())
            .ok_or_else(|| {
                ServiceError::Internal(format!("page `{name}` missing head snapshot"))
            })?;
        if let Some(spec) = page.spec.as_mut() {
            spec.publish = true;
            spec.publish_time.get_or_insert_with(Utc::now);
            spec.release_snapshot = Some(head.clone());
        }
        page.metadata.set_label(PUBLISHED_LABEL, "true");
        page.metadata.set_annotation(LAST_RELEASED_ANNO, &head);
        let saved = store.update(&page).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    pub async fn unpublish(&self, name: &str) -> Result<PageDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        if let Some(spec) = page.spec.as_mut() {
            spec.publish = false;
        }
        page.metadata.set_label(PUBLISHED_LABEL, "false");
        let saved = store.update(&page).await?;
        upsert(&self.index, &saved)?;
        self.detail_from_store(saved.metadata.name()).await
    }

    pub async fn increment_visit(&self, name: &str) -> Result<i32, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        let next = visits_from_page(&page).saturating_add(1);
        let mut stats = page
            .metadata
            .annotation(STATS_ANNO)
            .and_then(|raw| {
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(raw).ok()
            })
            .unwrap_or_default();
        stats.insert(
            "visit".to_owned(),
            serde_json::Value::Number(serde_json::Number::from(next)),
        );
        page.metadata
            .set_annotation(STATS_ANNO, serde_json::Value::Object(stats).to_string());
        let saved = store.update(&page).await?;
        upsert(&self.index, &saved)?;
        i32::try_from(next)
            .map_err(|e| ServiceError::Internal(format!("visit count overflow: {e}")))
    }

    async fn detail_from_store(&self, name: &str) -> Result<PageDetail, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let page: SinglePage = store
            .fetch::<SinglePage>(name)
            .await?
            .ok_or_else(|| not_found("SinglePage", name))?;
        let Some(head) = page
            .spec
            .as_ref()
            .and_then(|s| s.head_snapshot.clone())
            .or_else(|| page.spec.as_ref().and_then(|s| s.base_snapshot.clone()))
        else {
            return self.build_detail(page, "", "", "", "markdown");
        };
        let base_name = page
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
        self.build_detail(
            page,
            &rendered.html,
            &rendered.excerpt,
            &wrap.raw,
            &wrap.raw_type,
        )
    }

    fn build_detail(
        &self,
        page: SinglePage,
        rendered_html: &str,
        excerpt: &str,
        raw_markdown: &str,
        raw_type: &str,
    ) -> Result<PageDetail, ServiceError> {
        let spec = page
            .spec
            .clone()
            .ok_or_else(|| ServiceError::Internal("SinglePage missing spec".to_owned()))?;
        let status = page.status.as_ref();
        let status_excerpt = status.and_then(|s| non_empty(s.excerpt.as_deref()));
        let excerpt = non_empty(spec.excerpt.raw.as_deref())
            .or(status_excerpt)
            .unwrap_or(excerpt)
            .to_owned();
        let published = page
            .metadata
            .label(PUBLISHED_LABEL)
            .map(|v| v == "true")
            .unwrap_or(spec.publish);
        let deleted = page
            .metadata
            .label(DELETED_LABEL)
            .is_some_and(|v| v == "true")
            || page.metadata.is_deleted()
            || spec.deleted;
        let comments_count = self.comment_count_for_page(page.metadata.name())?;
        let visits = i32::try_from(visits_from_page(&page)).unwrap_or(i32::MAX);
        let image_count = count_images(rendered_html);
        Ok(PageDetail {
            name: page.metadata.name().to_owned(),
            title: spec.title,
            slug: spec.slug.clone(),
            permalink: permalink::page(&spec.slug),
            content_html: rendered_html.to_owned(),
            raw_markdown: raw_markdown.to_owned(),
            raw_type: raw_type.to_owned(),
            excerpt,
            publish_time: spec.publish_time,
            published,
            deleted,
            visible: spec.visible,
            owner: spec.owner,
            cover: spec.cover,
            template: spec.template,
            pinned: spec.pinned,
            allow_comment: spec.allow_comment,
            priority: spec.priority,
            visits,
            comments_count,
            image_count,
            creation_time: page.metadata.creation_timestamp,
            last_modify_time: status.and_then(|s| s.last_modify_time),
            metadata: page.metadata,
        })
    }

    fn comment_count_for_page(&self, page_name: &str) -> Result<i32, ServiceError> {
        let res = self.index.list(
            &Comment::gvk(),
            &ListOptions::default()
                .with_label(LabelSelector::Equals {
                    key: APPROVED_LABEL.to_owned(),
                    value: "true".to_owned(),
                })
                .with_label(LabelSelector::Equals {
                    key: SUBJECT_KIND_LABEL.to_owned(),
                    value: "SinglePage".to_owned(),
                })
                .with_label(LabelSelector::Equals {
                    key: SUBJECT_NAME_LABEL.to_owned(),
                    value: page_name.to_owned(),
                }),
        )?;
        let count = res
            .items
            .into_iter()
            .filter(|entry| {
                entry
                    .raw
                    .get("spec")
                    .and_then(|spec| spec.get("hidden"))
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
            })
            .count();
        i32::try_from(count)
            .map_err(|e| ServiceError::Internal(format!("comment count overflow: {e}")))
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PageSettingsUpdate {
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
pub struct PageListQuery {
    #[serde(default)]
    pub status: PageStatusFilter,
    #[serde(default)]
    pub include_deleted: bool,
    #[serde(default)]
    pub visible: Option<Visible>,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl Default for PageListQuery {
    fn default() -> Self {
        Self {
            status: PageStatusFilter::default(),
            include_deleted: false,
            visible: None,
            offset: default_offset(),
            limit: default_limit(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageStatusFilter {
    #[default]
    Published,
    Draft,
    Any,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageListItem {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub publish_time: Option<chrono::DateTime<Utc>>,
    pub excerpt: String,
    pub published: bool,
    pub visible: Visible,
    pub deleted: bool,
    pub creation_time: Option<chrono::DateTime<Utc>>,
    pub last_modify_time: Option<chrono::DateTime<Utc>>,
    pub comments_count: i32,
    pub visits: i32,
    pub image_count: usize,
    pub pinned: bool,
    pub priority: i32,
}

impl PageListItem {
    fn from_detail(detail: &PageDetail) -> Self {
        Self {
            name: detail.name.clone(),
            title: detail.title.clone(),
            slug: detail.slug.clone(),
            permalink: detail.permalink.clone(),
            publish_time: detail.publish_time,
            excerpt: detail.excerpt.clone(),
            published: detail.published,
            visible: detail.visible.clone(),
            deleted: detail.deleted,
            creation_time: detail.creation_time,
            last_modify_time: detail.last_modify_time.or(detail.creation_time),
            comments_count: detail.comments_count,
            visits: detail.visits,
            image_count: detail.image_count,
            pinned: detail.pinned,
            priority: detail.priority,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageListPage {
    pub items: Vec<PageListItem>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageDetail {
    pub name: String,
    pub title: String,
    pub slug: String,
    pub permalink: String,
    pub content_html: String,
    pub raw_markdown: String,
    pub raw_type: String,
    pub excerpt: String,
    pub publish_time: Option<chrono::DateTime<Utc>>,
    pub published: bool,
    pub deleted: bool,
    pub visible: Visible,
    pub owner: Option<String>,
    pub cover: Option<String>,
    pub template: Option<String>,
    pub pinned: bool,
    pub allow_comment: bool,
    pub priority: i32,
    pub visits: i32,
    pub comments_count: i32,
    pub image_count: usize,
    pub creation_time: Option<chrono::DateTime<Utc>>,
    pub last_modify_time: Option<chrono::DateTime<Utc>>,
    #[serde(skip)]
    pub metadata: Metadata,
}

fn page_list_time(item: &PageListItem) -> Option<chrono::DateTime<Utc>> {
    item.publish_time
        .or(item.last_modify_time)
        .or(item.creation_time)
}

fn visits_from_page(page: &SinglePage) -> u64 {
    page.metadata
        .annotation(STATS_ANNO)
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|stats| stats.get("visit").and_then(serde_json::Value::as_u64))
        .unwrap_or_default()
}

fn count_images(html: &str) -> usize {
    html.match_indices("<img")
        .chain(html.match_indices("<IMG"))
        .count()
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
    20
}
