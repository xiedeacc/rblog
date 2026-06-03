use std::sync::Arc;

use chrono::{DateTime, Utc};
use rblog_content::content::Visible;
use rblog_content::render::{MarkdownPipeline, RenderOptions};
use rblog_scheme::Metadata;
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;

use crate::{not_found, permalink, ServiceError};

#[derive(Clone)]
pub struct PageService {
    pool: AnyPool,
    pipeline: Arc<MarkdownPipeline>,
}

impl PageService {
    pub fn new(
        pool: AnyPool,
        _index: Arc<rblog_index::IndexEngine>,
        pipeline: Arc<MarkdownPipeline>,
    ) -> Self {
        Self { pool, pipeline }
    }

    pub async fn list(&self, query: PageListQuery) -> Result<PageListPage, ServiceError> {
        let rows = sqlx::query(
            r#"
            SELECT name FROM pages
            WHERE (? = 1 OR deleted = 0)
              AND (? = 'any' OR (? = 'published' AND published = 1) OR (? = 'draft' AND published = 0))
              AND (? IS NULL OR visible = ?)
            ORDER BY pinned DESC, published ASC, COALESCE(publish_time, updated_at, created_at) DESC, name ASC
            "#,
        )
        .bind(if query.include_deleted { 1_i64 } else { 0_i64 })
        .bind(status_filter(query.status))
        .bind(status_filter(query.status))
        .bind(status_filter(query.status))
        .bind(query.visible.map(visible_to_str))
        .bind(query.visible.map(visible_to_str))
        .fetch_all(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let mut items = Vec::new();
        for row in rows {
            items.push(PageListItem::from_detail(
                &self.admin_detail(row.get("name")).await?,
            ));
        }
        let total = items.len();
        Ok(PageListPage {
            items: items
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .collect(),
            total,
        })
    }

    pub async fn admin_detail(&self, name: &str) -> Result<PageDetail, ServiceError> {
        let row = sqlx::query("SELECT * FROM pages WHERE name = ?")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?
            .ok_or_else(|| not_found("Page", name))?;
        detail_from_row(&self.pool, row).await
    }

    pub async fn by_slug(
        &self,
        slug: &str,
        include_private: bool,
    ) -> Result<PageDetail, ServiceError> {
        let row = sqlx::query(
            "SELECT * FROM pages WHERE slug = ? AND deleted = 0 AND published = 1 AND (? = 1 OR visible = 'PUBLIC')",
        )
        .bind(slug)
        .bind(if include_private { 1_i64 } else { 0_i64 })
        .fetch_optional(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| not_found("Page", slug))?;
        detail_from_row(&self.pool, row).await
    }

    pub async fn update_content(
        &self,
        name: &str,
        markdown: &str,
        author: &str,
    ) -> Result<PageDetail, ServiceError> {
        let rendered = self
            .pipeline
            .render(markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let res = sqlx::query(
            "UPDATE pages SET markdown = ?, html = ?, raw_type = 'markdown', owner = COALESCE(owner, ?), updated_at = ? WHERE name = ?",
        )
        .bind(markdown)
        .bind(&rendered.html)
        .bind(author)
        .bind(Utc::now().to_rfc3339())
        .bind(name)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Page", name));
        }
        self.admin_detail(name).await
    }

    pub async fn update_settings(
        &self,
        name: &str,
        settings: PageSettingsUpdate,
    ) -> Result<PageDetail, ServiceError> {
        let mut detail = self.admin_detail(name).await?;
        if let Some(title) = settings.title {
            detail.title = title;
        }
        if let Some(slug) = settings.slug {
            detail.slug = slug;
        }
        if let Some(excerpt) = settings.excerpt {
            detail.excerpt = excerpt;
        }
        if let Some(visible) = settings.visible {
            detail.visible = visible;
        }
        if let Some(cover) = settings.cover {
            detail.cover = non_empty(cover);
        }
        if let Some(template) = settings.template {
            detail.template = non_empty(template);
        }
        if let Some(priority) = settings.priority {
            detail.priority = priority;
        }
        if let Some(pinned) = settings.pinned {
            detail.pinned = pinned;
        }
        if let Some(allow_comment) = settings.allow_comment {
            detail.allow_comment = allow_comment;
        }
        if let Some(publish_time) = settings.publish_time {
            detail.publish_time = publish_time;
        }
        sqlx::query(
            r#"
            UPDATE pages
            SET title = ?, slug = ?, excerpt = ?, visible = ?, cover = ?, template = ?,
                priority = ?, pinned = ?, allow_comment = ?, publish_time = ?, updated_at = ?
            WHERE name = ?
            "#,
        )
        .bind(&detail.title)
        .bind(&detail.slug)
        .bind(&detail.excerpt)
        .bind(visible_to_str(detail.visible))
        .bind(detail.cover.as_deref())
        .bind(detail.template.as_deref())
        .bind(i64::from(detail.priority))
        .bind(if detail.pinned { 1_i64 } else { 0_i64 })
        .bind(if detail.allow_comment { 1_i64 } else { 0_i64 })
        .bind(detail.publish_time.map(|t| t.to_rfc3339()))
        .bind(Utc::now().to_rfc3339())
        .bind(name)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn publish(&self, name: &str) -> Result<PageDetail, ServiceError> {
        sqlx::query("UPDATE pages SET published = 1, publish_time = COALESCE(publish_time, ?), updated_at = ? WHERE name = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn unpublish(&self, name: &str) -> Result<PageDetail, ServiceError> {
        sqlx::query("UPDATE pages SET published = 0, updated_at = ? WHERE name = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn increment_visit(&self, name: &str) -> Result<i32, ServiceError> {
        let row =
            sqlx::query("UPDATE pages SET visits = visits + 1 WHERE name = ? RETURNING visits")
                .bind(name)
                .fetch_optional(sqlite(&self.pool)?)
                .await
                .map_err(map_sqlx)?;
        let Some(row) = row else {
            return Err(not_found("Page", name));
        };
        to_i32(row.get("visits"), "visits")
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
    pub publish_time: Option<Option<DateTime<Utc>>>,
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
    pub publish_time: Option<DateTime<Utc>>,
    pub excerpt: String,
    pub published: bool,
    pub visible: Visible,
    pub deleted: bool,
    pub creation_time: Option<DateTime<Utc>>,
    pub last_modify_time: Option<DateTime<Utc>>,
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
            visible: detail.visible,
            deleted: detail.deleted,
            creation_time: detail.creation_time,
            last_modify_time: detail.last_modify_time,
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
    pub publish_time: Option<DateTime<Utc>>,
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
    pub creation_time: Option<DateTime<Utc>>,
    pub last_modify_time: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub metadata: Metadata,
}

async fn detail_from_row(
    pool: &AnyPool,
    row: sqlx::sqlite::SqliteRow,
) -> Result<PageDetail, ServiceError> {
    let name: String = row.get("name");
    let slug: String = row.get("slug");
    let html: String = row.get("html");
    Ok(PageDetail {
        name: name.clone(),
        title: row.get("title"),
        slug: slug.clone(),
        permalink: permalink::page(&slug),
        content_html: html.clone(),
        raw_markdown: row.get("markdown"),
        raw_type: row.get("raw_type"),
        excerpt: row
            .try_get::<Option<String>, _>("excerpt")
            .ok()
            .flatten()
            .unwrap_or_default(),
        publish_time: parse_dt(
            row.try_get::<Option<String>, _>("publish_time")
                .ok()
                .flatten(),
        ),
        published: row.get::<i64, _>("published") != 0,
        deleted: row.get::<i64, _>("deleted") != 0,
        visible: parse_visible(&row.get::<String, _>("visible")),
        owner: row.try_get("owner").ok().flatten(),
        cover: row.try_get("cover").ok().flatten(),
        template: row.try_get("template").ok().flatten(),
        pinned: row.get::<i64, _>("pinned") != 0,
        allow_comment: row.get::<i64, _>("allow_comment") != 0,
        priority: to_i32(row.get("priority"), "priority")?,
        visits: to_i32(row.get("visits"), "visits")?,
        comments_count: comment_count(pool, &name).await?,
        image_count: html.matches("<img").count(),
        creation_time: parse_dt(
            row.try_get::<Option<String>, _>("created_at")
                .ok()
                .flatten(),
        ),
        last_modify_time: parse_dt(
            row.try_get::<Option<String>, _>("updated_at")
                .ok()
                .flatten(),
        ),
        metadata: Metadata {
            name,
            ..Metadata::default()
        },
    })
}

async fn comment_count(pool: &AnyPool, name: &str) -> Result<i32, ServiceError> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS count FROM comments WHERE subject_kind = 'SinglePage' AND subject_name = ? AND approved = 1 AND hidden = 0 AND parent_name IS NULL",
    )
    .bind(name)
    .fetch_one(sqlite(pool)?)
    .await
    .map_err(map_sqlx)?;
    to_i32(row.get("count"), "comment count")
}

fn default_offset() -> usize {
    0
}
fn default_limit() -> usize {
    20
}

fn status_filter(status: PageStatusFilter) -> &'static str {
    match status {
        PageStatusFilter::Published => "published",
        PageStatusFilter::Draft => "draft",
        PageStatusFilter::Any => "any",
    }
}

fn visible_to_str(value: Visible) -> &'static str {
    match value {
        Visible::Public => "PUBLIC",
        Visible::Internal => "INTERNAL",
        Visible::Private => "PRIVATE",
    }
}

fn parse_visible(value: &str) -> Visible {
    match value {
        "PRIVATE" => Visible::Private,
        "INTERNAL" => Visible::Internal,
        _ => Visible::Public,
    }
}

fn parse_dt(raw: Option<String>) -> Option<DateTime<Utc>> {
    raw.and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn to_i32(value: i64, field: &str) -> Result<i32, ServiceError> {
    i32::try_from(value).map_err(|e| ServiceError::Internal(format!("{field} overflow: {e}")))
}

fn sqlite(pool: &AnyPool) -> Result<&sqlx::SqlitePool, ServiceError> {
    match pool {
        AnyPool::Sqlite(pool) => Ok(pool),
        AnyPool::Mysql(_) => Err(ServiceError::Internal(
            "refactor branch only supports sqlite".to_owned(),
        )),
    }
}

fn map_sqlx(error: sqlx::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
}
