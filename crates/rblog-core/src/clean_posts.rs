//! Post service backed by first-class rblog tables.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use rblog_content::content::Visible;
use rblog_content::render::{MarkdownPipeline, RenderOptions};
use rblog_scheme::Metadata;
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;

use crate::permalink;
use crate::ServiceError;

#[derive(Clone)]
pub struct PostService {
    pool: AnyPool,
    pipeline: Arc<MarkdownPipeline>,
}

impl PostService {
    pub fn new(
        pool: AnyPool,
        _index: Arc<rblog_index::IndexEngine>,
        pipeline: Arc<MarkdownPipeline>,
    ) -> Self {
        Self { pool, pipeline }
    }

    pub async fn draft(&self, draft: DraftPost) -> Result<PostDetail, ServiceError> {
        if draft.title.trim().is_empty() {
            return Err(ServiceError::Validation("title must not be empty".into()));
        }
        if draft.slug.trim().is_empty() {
            return Err(ServiceError::Validation("slug must not be empty".into()));
        }
        self.ensure_slug_available(&draft.slug, None).await?;
        let rendered = self
            .pipeline
            .render(&draft.markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let now = Utc::now();
        let excerpt = draft
            .excerpt
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(rendered.excerpt.clone()));
        let pool = sqlite(&self.pool)?;
        sqlx::query(
            r#"
            INSERT INTO posts (
                name, title, slug, markdown, html, raw_type, excerpt, owner, cover, template,
                published, visible, deleted, pinned, allow_comment, priority, publish_time,
                created_at, updated_at, visits
            )
            VALUES (?, ?, ?, ?, ?, 'markdown', ?, ?, ?, ?, 0, ?, 0, ?, ?, ?, NULL, ?, ?, 0)
            "#,
        )
        .bind(&draft.name)
        .bind(&draft.title)
        .bind(&draft.slug)
        .bind(&draft.markdown)
        .bind(&rendered.html)
        .bind(excerpt.as_deref())
        .bind(&draft.owner)
        .bind(draft.cover.as_deref())
        .bind(draft.template.as_deref())
        .bind(visible_to_str(draft.visible))
        .bind(bool_to_i64(draft.pinned.unwrap_or(false)))
        .bind(bool_to_i64(draft.allow_comment.unwrap_or(true)))
        .bind(i64::from(draft.priority.unwrap_or(0)))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        self.replace_terms(
            &draft.name,
            "post_tags",
            "tag_name",
            draft.tags.unwrap_or_default(),
        )
        .await?;
        self.replace_terms(
            &draft.name,
            "post_categories",
            "category_name",
            draft.categories.unwrap_or_default(),
        )
        .await?;
        self.admin_detail(&draft.name).await
    }

    pub async fn update_content(
        &self,
        name: &str,
        markdown: &str,
        _author: &str,
    ) -> Result<PostDetail, ServiceError> {
        let rendered = self
            .pipeline
            .render(markdown, &RenderOptions::default())
            .map_err(|e| ServiceError::Content(e.to_string()))?;
        let now = Utc::now();
        let pool = sqlite(&self.pool)?;
        let res = sqlx::query(
            "UPDATE posts SET markdown = ?, html = ?, raw_type = 'markdown', excerpt = ?, updated_at = ? WHERE name = ?",
        )
        .bind(markdown)
        .bind(&rendered.html)
        .bind(&rendered.excerpt)
        .bind(now.to_rfc3339())
        .bind(name)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(crate::not_found("Post", name));
        }
        self.admin_detail(name).await
    }

    pub async fn backfill_missing_excerpts(&self) -> Result<usize, ServiceError> {
        let rows = sqlx::query(
            "SELECT name, markdown, html FROM posts WHERE excerpt IS NULL OR TRIM(excerpt) = ''",
        )
        .fetch_all(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let mut updated = 0;
        for row in rows {
            let name: String = row.get("name");
            let markdown: String = row.get("markdown");
            let html: String = row.get("html");
            let excerpt = excerpt_from_content(&markdown, &html);
            if excerpt.trim().is_empty() {
                continue;
            }
            sqlx::query("UPDATE posts SET excerpt = ?, updated_at = updated_at WHERE name = ?")
                .bind(excerpt)
                .bind(name)
                .execute(sqlite(&self.pool)?)
                .await
                .map_err(map_sqlx)?;
            updated += 1;
        }
        Ok(updated)
    }

    pub async fn update_settings(
        &self,
        name: &str,
        settings: PostSettingsUpdate,
    ) -> Result<PostDetail, ServiceError> {
        let mut detail = self.admin_detail(name).await?;
        if let Some(title) = settings.title {
            if title.trim().is_empty() {
                return Err(ServiceError::Validation("title must not be empty".into()));
            }
            detail.title = title;
        }
        if let Some(slug) = settings.slug {
            if slug.trim().is_empty() {
                return Err(ServiceError::Validation("slug must not be empty".into()));
            }
            self.ensure_slug_available(&slug, Some(name)).await?;
            detail.slug = slug;
        }
        if let Some(excerpt) = settings.excerpt {
            detail.excerpt = if excerpt.trim().is_empty() {
                excerpt_from_content(&detail.raw_markdown, &detail.content_html)
            } else {
                excerpt
            };
        }
        if let Some(visible) = settings.visible {
            detail.visible = visible;
        }
        if let Some(cover) = settings.cover {
            detail.cover = non_empty_owned(cover);
        }
        if let Some(template) = settings.template {
            detail.template = non_empty_owned(template);
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
        let now = Utc::now();
        let pool = sqlite(&self.pool)?;
        sqlx::query(
            r#"
            UPDATE posts
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
        .bind(bool_to_i64(detail.pinned))
        .bind(bool_to_i64(detail.allow_comment))
        .bind(detail.publish_time.map(|t| t.to_rfc3339()))
        .bind(now.to_rfc3339())
        .bind(name)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn increment_visit(&self, name: &str) -> Result<i32, ServiceError> {
        let pool = sqlite(&self.pool)?;
        let res = sqlx::query("UPDATE posts SET visits = visits + 1 WHERE name = ?")
            .bind(name)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(crate::not_found("Post", name));
        }
        let row = sqlx::query("SELECT visits FROM posts WHERE name = ?")
            .bind(name)
            .fetch_one(pool)
            .await
            .map_err(map_sqlx)?;
        to_i32(row.get::<i64, _>("visits"), "visit count")
    }

    pub async fn publish(
        &self,
        name: &str,
        opts: PublishOptions,
    ) -> Result<PostDetail, ServiceError> {
        let publish_time = opts.publish_time.unwrap_or_else(Utc::now);
        let pool = sqlite(&self.pool)?;
        let visible = opts.visible.map(visible_to_str);
        if let Some(visible) = visible {
            sqlx::query("UPDATE posts SET published = 1, visible = ?, publish_time = COALESCE(publish_time, ?), updated_at = ? WHERE name = ?")
                .bind(visible)
                .bind(publish_time.to_rfc3339())
                .bind(Utc::now().to_rfc3339())
                .bind(name)
                .execute(pool)
                .await
                .map_err(map_sqlx)?;
        } else {
            sqlx::query("UPDATE posts SET published = 1, publish_time = COALESCE(publish_time, ?), updated_at = ? WHERE name = ?")
                .bind(publish_time.to_rfc3339())
                .bind(Utc::now().to_rfc3339())
                .bind(name)
                .execute(pool)
                .await
                .map_err(map_sqlx)?;
        }
        self.admin_detail(name).await
    }

    pub async fn unpublish(&self, name: &str) -> Result<PostDetail, ServiceError> {
        sqlx::query("UPDATE posts SET published = 0, updated_at = ? WHERE name = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn soft_delete(&self, name: &str) -> Result<(), ServiceError> {
        let now = Utc::now();
        sqlx::query("UPDATE posts SET deleted = 1, published = 0, deleted_at = ?, updated_at = ? WHERE name = ?")
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn restore(&self, name: &str) -> Result<PostDetail, ServiceError> {
        sqlx::query(
            "UPDATE posts SET deleted = 0, deleted_at = NULL, updated_at = ? WHERE name = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(name)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        self.admin_detail(name).await
    }

    pub async fn purge_expired_deleted(&self) -> Result<usize, ServiceError> {
        Ok(0)
    }

    pub async fn purge(&self, name: &str) -> Result<(), ServiceError> {
        let pool = sqlite(&self.pool)?;
        sqlx::query("DELETE FROM posts WHERE name = ?")
            .bind(name)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        Ok(())
    }

    pub async fn list(&self, query: PostListQuery) -> Result<PostListPage, ServiceError> {
        let pool = sqlite(&self.pool)?;
        let mut rows = sqlx::query(
            r#"
            SELECT name FROM posts
            WHERE (? = 1 OR deleted = 0)
              AND (? = 0 OR deleted = 1)
              AND (? = 'any' OR (? = 'published' AND published = 1) OR (? = 'draft' AND published = 0))
              AND (? IS NULL OR visible = ?)
              AND (? = 0 OR visible = 'PUBLIC')
            ORDER BY pinned DESC, published ASC, COALESCE(publish_time, updated_at, created_at) DESC, name ASC
            "#,
        )
        .bind(bool_to_i64(query.include_deleted))
        .bind(bool_to_i64(query.deleted_only))
        .bind(status_filter(query.status))
        .bind(status_filter(query.status))
        .bind(status_filter(query.status))
        .bind(query.visible.map(visible_to_str))
        .bind(query.visible.map(visible_to_str))
        .bind(bool_to_i64(query.public_only))
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?;
        let mut items = Vec::new();
        for row in rows.drain(..) {
            let name: String = row.get("name");
            let detail = self.admin_detail(&name).await?;
            if query
                .tag
                .as_deref()
                .is_none_or(|tag| detail.tags.iter().any(|item| item == tag))
                && query
                    .category
                    .as_deref()
                    .is_none_or(|cat| detail.categories.iter().any(|item| item == cat))
            {
                items.push(PostListItem::from_detail(&detail));
            }
        }
        let total = items.len();
        let items = items
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect();
        Ok(PostListPage { items, total })
    }

    pub async fn public_detail(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let detail = self.admin_detail(name).await?;
        if !detail.published || detail.deleted || detail.visible != Visible::Public {
            return Err(crate::not_found("Post", name));
        }
        Ok(detail)
    }

    pub async fn admin_detail(&self, name: &str) -> Result<PostDetail, ServiceError> {
        let row = sqlx::query("SELECT * FROM posts WHERE name = ?")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?
            .ok_or_else(|| crate::not_found("Post", name))?;
        self.detail_from_row(row).await
    }

    pub async fn public_by_slug(&self, slug: &str) -> Result<PostDetail, ServiceError> {
        self.by_slug(slug, false).await
    }

    pub async fn by_slug(
        &self,
        slug: &str,
        include_private: bool,
    ) -> Result<PostDetail, ServiceError> {
        let row = sqlx::query(
            "SELECT * FROM posts WHERE slug = ? AND deleted = 0 AND published = 1 AND (? = 1 OR visible = 'PUBLIC')",
        )
        .bind(slug)
        .bind(bool_to_i64(include_private))
        .fetch_optional(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?
        .ok_or_else(|| crate::not_found("Post", slug))?;
        self.detail_from_row(row).await
    }

    async fn detail_from_row(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<PostDetail, ServiceError> {
        let name: String = row.get("name");
        let tags = terms(&self.pool, "post_tags", "tag_name", &name).await?;
        let categories = terms(&self.pool, "post_categories", "category_name", &name).await?;
        let comments_count = comment_count(&self.pool, "Post", &name).await?;
        let slug: String = row.get("slug");
        let html: String = row.get("html");
        let markdown: String = row.get("markdown");
        let stored_excerpt = row
            .try_get::<Option<String>, _>("excerpt")
            .unwrap_or_default()
            .unwrap_or_default();
        let excerpt = if stored_excerpt.trim().is_empty() {
            excerpt_from_content(&markdown, &html)
        } else {
            stored_excerpt
        };
        Ok(PostDetail {
            name: name.clone(),
            title: row.get("title"),
            slug: slug.clone(),
            permalink: permalink::post(&slug),
            content_html: html,
            raw_markdown: markdown,
            raw_type: row.get("raw_type"),
            excerpt,
            publish_time: parse_dt(
                row.try_get::<Option<String>, _>("publish_time")
                    .ok()
                    .flatten(),
            ),
            published: row.get::<i64, _>("published") != 0,
            deleted: row.get::<i64, _>("deleted") != 0,
            visible: parse_visible(row.get::<String, _>("visible").as_str()),
            owner: row.try_get("owner").ok().flatten(),
            categories,
            tags,
            cover: row.try_get("cover").ok().flatten(),
            template: row.try_get("template").ok().flatten(),
            pinned: row.get::<i64, _>("pinned") != 0,
            allow_comment: row.get::<i64, _>("allow_comment") != 0,
            priority: to_i32(row.get::<i64, _>("priority"), "priority")?,
            visits: to_i32(row.get::<i64, _>("visits"), "visits")?,
            comments_count,
            last_modify_time: parse_dt(
                row.try_get::<Option<String>, _>("updated_at")
                    .ok()
                    .flatten(),
            ),
            metadata: Metadata {
                name,
                creation_timestamp: parse_dt(
                    row.try_get::<Option<String>, _>("created_at")
                        .ok()
                        .flatten(),
                ),
                deletion_timestamp: parse_dt(
                    row.try_get::<Option<String>, _>("deleted_at")
                        .ok()
                        .flatten(),
                ),
                ..Metadata::default()
            },
        })
    }

    async fn replace_terms(
        &self,
        post_name: &str,
        table: &str,
        column: &str,
        values: Vec<String>,
    ) -> Result<(), ServiceError> {
        let pool = sqlite(&self.pool)?;
        sqlx::query(&format!("DELETE FROM {table} WHERE post_name = ?"))
            .bind(post_name)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        for value in values {
            match table {
                "post_tags" => {
                    sqlx::query(
                        "INSERT OR IGNORE INTO tags (name, display_name, slug, color, created_at, updated_at) VALUES (?, ?, ?, NULL, ?, ?)",
                    )
                    .bind(&value)
                    .bind(&value)
                    .bind(&value)
                    .bind(Utc::now().to_rfc3339())
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await
                    .map_err(map_sqlx)?;
                }
                "post_categories" => {
                    sqlx::query(
                        "INSERT OR IGNORE INTO categories (name, display_name, slug, description, cover, template, priority, created_at, updated_at) VALUES (?, ?, ?, NULL, NULL, NULL, 0, ?, ?)",
                    )
                    .bind(&value)
                    .bind(&value)
                    .bind(&value)
                    .bind(Utc::now().to_rfc3339())
                    .bind(Utc::now().to_rfc3339())
                    .execute(pool)
                    .await
                    .map_err(map_sqlx)?;
                }
                _ => {}
            }
            sqlx::query(&format!(
                "INSERT OR IGNORE INTO {table} (post_name, {column}) VALUES (?, ?)"
            ))
            .bind(post_name)
            .bind(value)
            .execute(pool)
            .await
            .map_err(map_sqlx)?;
        }
        Ok(())
    }

    async fn ensure_slug_available(
        &self,
        slug: &str,
        current_name: Option<&str>,
    ) -> Result<(), ServiceError> {
        let row = sqlx::query("SELECT name FROM posts WHERE slug = ?")
            .bind(slug)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if row
            .as_ref()
            .is_some_and(|row| current_name != Some(row.get::<String, _>("name").as_str()))
        {
            return Err(crate::conflict("Post slug", slug));
        }
        Ok(())
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
    pub publish_time: Option<DateTime<Utc>>,
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
    pub publish_time: Option<Option<DateTime<Utc>>>,
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
    pub publish_time: Option<DateTime<Utc>>,
    pub excerpt: Option<String>,
    pub tags: Vec<String>,
    pub categories: Vec<String>,
    pub cover: Option<String>,
    pub published: bool,
    pub visible: Visible,
    pub deleted: bool,
    pub deletion_time: Option<DateTime<Utc>>,
    pub creation_time: Option<DateTime<Utc>>,
    pub last_modify_time: Option<DateTime<Utc>>,
    pub comments_count: i32,
    pub visits: i32,
    pub pinned: bool,
    pub priority: i32,
}

impl PostListItem {
    fn from_detail(detail: &PostDetail) -> Self {
        Self {
            name: detail.name.clone(),
            title: detail.title.clone(),
            slug: detail.slug.clone(),
            permalink: detail.permalink.clone(),
            publish_time: detail.publish_time,
            excerpt: non_empty_owned(detail.excerpt.clone()),
            tags: detail.tags.clone(),
            categories: detail.categories.clone(),
            cover: detail.cover.clone(),
            published: detail.published,
            visible: detail.visible,
            deleted: detail.deleted,
            deletion_time: detail.metadata.deletion_timestamp,
            creation_time: detail.metadata.creation_timestamp,
            last_modify_time: detail.last_modify_time,
            comments_count: detail.comments_count,
            visits: detail.visits,
            pinned: detail.pinned,
            priority: detail.priority,
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
    pub raw_type: String,
    pub excerpt: String,
    pub publish_time: Option<DateTime<Utc>>,
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
    pub comments_count: i32,
    pub last_modify_time: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub metadata: Metadata,
}

fn default_offset() -> usize {
    0
}

fn default_limit() -> usize {
    10
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

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
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

fn status_filter(status: PostStatusFilter) -> &'static str {
    match status {
        PostStatusFilter::Published => "published",
        PostStatusFilter::Draft => "draft",
        PostStatusFilter::Any => "any",
    }
}

fn parse_dt(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn excerpt_from_content(markdown: &str, html: &str) -> String {
    let cleaned_markdown;
    let source = if markdown.trim().is_empty() {
        html
    } else {
        cleaned_markdown = strip_markdown_images(markdown);
        &cleaned_markdown
    };
    let mut text = String::with_capacity(source.len().min(512));
    let mut in_tag = false;
    let mut in_entity = false;
    for ch in source.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            '&' if !in_tag => in_entity = true,
            ';' if in_entity => {
                in_entity = false;
                text.push(' ');
            }
            _ if in_tag || in_entity => {}
            '#' | '*' | '_' | '`' | '[' | ']' | '(' | ')' | '>' | '-' => text.push(' '),
            _ => text.push(ch),
        }
    }
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(180).collect()
}

fn strip_markdown_images(markdown: &str) -> String {
    let mut cleaned = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(start) = rest.find("![") {
        cleaned.push_str(&rest[..start]);
        let candidate = &rest[start + 2..];
        let Some(alt_end) = candidate.find(']') else {
            cleaned.push_str(&rest[start..]);
            return cleaned;
        };
        let after_alt = &candidate[alt_end + 1..];
        if !after_alt.starts_with('(') {
            cleaned.push_str(&rest[start..start + 2 + alt_end + 1]);
            rest = after_alt;
            continue;
        }
        let Some(url_end) = after_alt[1..].find(')') else {
            cleaned.push_str(&rest[start..]);
            return cleaned;
        };
        rest = &after_alt[url_end + 2..];
    }
    cleaned.push_str(rest);
    cleaned
}

fn non_empty_owned(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn to_i32(value: i64, field: &str) -> Result<i32, ServiceError> {
    i32::try_from(value).map_err(|e| ServiceError::Internal(format!("{field} overflow: {e}")))
}

async fn terms(
    pool: &AnyPool,
    table: &str,
    column: &str,
    post_name: &str,
) -> Result<Vec<String>, ServiceError> {
    let rows = sqlx::query(&format!(
        "SELECT {column} AS value FROM {table} WHERE post_name = ? ORDER BY {column}"
    ))
    .bind(post_name)
    .fetch_all(sqlite(pool)?)
    .await
    .map_err(map_sqlx)?;
    Ok(rows.into_iter().map(|row| row.get("value")).collect())
}

async fn comment_count(pool: &AnyPool, kind: &str, name: &str) -> Result<i32, ServiceError> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS count FROM comments WHERE subject_kind = ? AND subject_name = ? AND approved = 1 AND hidden = 0 AND parent_name IS NULL",
    )
    .bind(kind)
    .bind(name)
    .fetch_one(sqlite(pool)?)
    .await
    .map_err(map_sqlx)?;
    to_i32(row.get::<i64, _>("count"), "comment count")
}
