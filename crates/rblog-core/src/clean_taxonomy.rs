use std::sync::Arc;

use rblog_content::content::{Category, CategorySpec, Tag, TagSpec};
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;

use crate::{conflict, not_found, permalink, ServiceError};

#[derive(Clone)]
pub struct TagService {
    pool: AnyPool,
    index: Arc<rblog_index::IndexEngine>,
}

impl TagService {
    pub fn new(pool: AnyPool, index: Arc<rblog_index::IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create(&self, new: NewTag) -> Result<Tag, ServiceError> {
        validate_slug(&new.slug)?;
        if self.find_tag(&new.name).await?.is_some() {
            return Err(conflict("Tag", new.name));
        }
        sqlx::query(
            "INSERT INTO tags (name, display_name, slug, color, cover, created_at, updated_at) VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(&new.name)
        .bind(&new.display_name)
        .bind(&new.slug)
        .bind(new.color.as_deref())
        .bind(new.cover.as_deref())
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let tag = self.get(&new.name).await?;
        crate::indexing::upsert(&self.index, &tag)?;
        Ok(tag)
    }

    pub async fn update(&self, tag: &Tag) -> Result<Tag, ServiceError> {
        let spec = tag.spec.clone().unwrap_or_default();
        validate_slug(&spec.slug)?;
        let res = sqlx::query(
            "UPDATE tags SET display_name = ?, slug = ?, color = ?, cover = ?, updated_at = CURRENT_TIMESTAMP WHERE name = ?",
        )
        .bind(&spec.display_name)
        .bind(&spec.slug)
        .bind(spec.color.as_deref())
        .bind(spec.cover.as_deref())
        .bind(&tag.metadata.name)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Tag", &tag.metadata.name));
        }
        let saved = self.get(&tag.metadata.name).await?;
        crate::indexing::upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM tags WHERE name = ?")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Tag", name));
        }
        crate::indexing::remove::<Tag>(&self.index, name);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Result<Tag, ServiceError> {
        self.find_tag(name)
            .await?
            .ok_or_else(|| not_found("Tag", name))
    }

    pub async fn stats(&self) -> Result<Vec<TagStats>, ServiceError> {
        let rows = sqlx::query(
            "SELECT t.*, COUNT(p.name) AS post_count FROM tags t LEFT JOIN post_tags pt ON pt.tag_name = t.name LEFT JOIN posts p ON p.name = pt.post_name AND p.published = 1 AND p.deleted = 0 AND p.visible = 'PUBLIC' GROUP BY t.name ORDER BY t.name",
        )
        .fetch_all(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let mut out = rows
            .into_iter()
            .map(|row| {
                let slug: String = row.get("slug");
                Ok(TagStats {
                    name: row.get("name"),
                    display_name: row.get("display_name"),
                    slug: slug.clone(),
                    permalink: permalink::tag(&slug),
                    color: row.try_get("color").ok().flatten(),
                    post_count: usize::try_from(row.get::<i64, _>("post_count"))
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn find_tag(&self, name: &str) -> Result<Option<Tag>, ServiceError> {
        let row = sqlx::query("SELECT * FROM tags WHERE name = ?")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        Ok(row.map(tag_from_row))
    }
}

#[derive(Clone)]
pub struct CategoryService {
    pool: AnyPool,
    index: Arc<rblog_index::IndexEngine>,
}

impl CategoryService {
    pub fn new(pool: AnyPool, index: Arc<rblog_index::IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create(&self, new: NewCategory) -> Result<Category, ServiceError> {
        validate_slug(&new.slug)?;
        if self.find_category(&new.name).await?.is_some() {
            return Err(conflict("Category", new.name));
        }
        sqlx::query(
            "INSERT INTO categories (name, display_name, slug, description, cover, template, priority, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        )
        .bind(&new.name)
        .bind(&new.display_name)
        .bind(&new.slug)
        .bind(new.description.as_deref())
        .bind(new.cover.as_deref())
        .bind(new.template.as_deref())
        .bind(i64::from(new.priority))
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let category = self.get(&new.name).await?;
        crate::indexing::upsert(&self.index, &category)?;
        Ok(category)
    }

    pub async fn update(&self, category: &Category) -> Result<Category, ServiceError> {
        let spec = category.spec.clone().unwrap_or_default();
        validate_slug(&spec.slug)?;
        let res = sqlx::query(
            "UPDATE categories SET display_name = ?, slug = ?, description = ?, cover = ?, template = ?, priority = ?, updated_at = CURRENT_TIMESTAMP WHERE name = ?",
        )
        .bind(&spec.display_name)
        .bind(&spec.slug)
        .bind(spec.description.as_deref())
        .bind(spec.cover.as_deref())
        .bind(spec.template.as_deref())
        .bind(i64::from(spec.priority))
        .bind(&category.metadata.name)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Category", &category.metadata.name));
        }
        let saved = self.get(&category.metadata.name).await?;
        crate::indexing::upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM categories WHERE name = ?")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Category", name));
        }
        crate::indexing::remove::<Category>(&self.index, name);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Result<Category, ServiceError> {
        self.find_category(name)
            .await?
            .ok_or_else(|| not_found("Category", name))
    }

    pub async fn stats(&self) -> Result<Vec<CategoryStats>, ServiceError> {
        let rows = sqlx::query(
            "SELECT c.*, COUNT(p.name) AS post_count FROM categories c LEFT JOIN post_categories pc ON pc.category_name = c.name LEFT JOIN posts p ON p.name = pc.post_name AND p.published = 1 AND p.deleted = 0 AND p.visible = 'PUBLIC' GROUP BY c.name ORDER BY c.priority, c.name",
        )
        .fetch_all(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let mut out = rows
            .into_iter()
            .map(|row| {
                let slug: String = row.get("slug");
                Ok(CategoryStats {
                    name: row.get("name"),
                    display_name: row.get("display_name"),
                    slug: slug.clone(),
                    permalink: permalink::category(&slug),
                    priority: i32::try_from(row.get::<i64, _>("priority")).unwrap_or_default(),
                    post_count: usize::try_from(row.get::<i64, _>("post_count"))
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?;
        out.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));
        Ok(out)
    }

    async fn find_category(&self, name: &str) -> Result<Option<Category>, ServiceError> {
        let row = sqlx::query("SELECT * FROM categories WHERE name = ?")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        Ok(row.map(category_from_row))
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewTag {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub cover: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewCategory {
    pub name: String,
    pub display_name: String,
    pub slug: String,
    pub description: Option<String>,
    pub cover: Option<String>,
    pub template: Option<String>,
    pub post_template: Option<String>,
    #[serde(default)]
    pub priority: i32,
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

fn tag_from_row(row: sqlx::sqlite::SqliteRow) -> Tag {
    Tag::new(row.get::<String, _>("name")).with_spec(TagSpec {
        display_name: row.get("display_name"),
        slug: row.get("slug"),
        description: None,
        color: row.try_get("color").ok().flatten(),
        cover: row.try_get("cover").ok().flatten(),
    })
}

fn category_from_row(row: sqlx::sqlite::SqliteRow) -> Category {
    Category::new(row.get::<String, _>("name")).with_spec(CategorySpec {
        display_name: row.get("display_name"),
        slug: row.get("slug"),
        description: row.try_get("description").ok().flatten(),
        cover: row.try_get("cover").ok().flatten(),
        template: row.try_get("template").ok().flatten(),
        post_template: None,
        priority: i32::try_from(row.get::<i64, _>("priority")).unwrap_or_default(),
        children: None,
        prevent_parent_post_cascade_query: false,
        hide_from_list: false,
    })
}

fn validate_slug(slug: &str) -> Result<(), ServiceError> {
    if slug.trim().is_empty() {
        return Err(ServiceError::Validation("slug must not be empty".into()));
    }
    if slug.contains('/') {
        return Err(ServiceError::Validation("slug must not contain `/`".into()));
    }
    Ok(())
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
