//! System integration helpers: index resync and the `Services` builder.

use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_content::content::{
    BaseCommentSpec, Category, CategorySpec, Comment, CommentOwner, CommentSpec, Reply, ReplySpec,
    Tag, TagSpec,
};
use rblog_content::core::ConfigMap;
use rblog_content::infra::Ref;
use rblog_content::render::MarkdownPipeline;
use rblog_index::IndexEngine;
use rblog_scheme::Extension;
use rblog_store::AnyPool;
use sqlx::Row;

use crate::clean_settings::SYSTEM_CONFIGMAP;
use crate::comments::{APPROVED_LABEL, SUBJECT_KIND_LABEL, SUBJECT_NAME_LABEL};
use crate::indexing::upsert;
use crate::{
    CategoryService, CommentService, ConfigMapService, PageService, PostService, ServiceError,
    Services, SettingService, TagService, UserService,
};

/// Build the bundle of services. Constructs an [`IndexEngine`] and resyncs
/// it from the live store before returning, so callers can serve requests
/// immediately.
pub async fn build_services(
    pool: AnyPool,
    pipeline: Arc<MarkdownPipeline>,
    hasher: Arc<PasswordHasher>,
) -> Result<Services, ServiceError> {
    let index = Arc::new(IndexEngine::new());
    resync_all(&index, &pool).await?;
    let posts = Arc::new(PostService::new(
        pool.clone(),
        index.clone(),
        pipeline.clone(),
    ));
    let pages = Arc::new(PageService::new(
        pool.clone(),
        index.clone(),
        pipeline.clone(),
    ));
    let tags = Arc::new(TagService::new(pool.clone(), index.clone()));
    let categories = Arc::new(CategoryService::new(pool.clone(), index.clone()));
    let comments = Arc::new(CommentService::new(pool.clone(), index.clone()));
    let users = Arc::new(UserService::new(
        pool.clone(),
        index.clone(),
        hasher.clone(),
    ));
    let settings = Arc::new(SettingService::new(pool.clone(), index.clone()));
    let configmaps = Arc::new(ConfigMapService::new(pool, index.clone()));
    Ok(Services {
        pages,
        posts,
        categories,
        tags,
        comments,
        users,
        settings,
        configmaps,
        hasher,
        index,
    })
}

/// Resync the runtime read projections from the clean relational tables.
pub async fn resync_all(index: &Arc<IndexEngine>, pool: &AnyPool) -> Result<(), ServiceError> {
    let pool = sqlite(pool)?;
    let mut data = std::collections::BTreeMap::new();
    for row in sqlx::query(
        "SELECT key, value FROM site_settings WHERE key NOT LIKE 'configmap.%' AND key NOT LIKE 'setting.%'",
    )
    .fetch_all(pool)
    .await
    .map_err(map_sqlx)?
    {
        data.insert(row.get("key"), row.try_get("value").ok().flatten().unwrap_or_default());
    }
    let mut cm = ConfigMap::new(SYSTEM_CONFIGMAP);
    cm.data = Some(data);
    upsert(index, &cm)?;

    for row in sqlx::query("SELECT * FROM tags")
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?
    {
        let tag = Tag::new(row.get::<String, _>("name")).with_spec(TagSpec {
            display_name: row.get("display_name"),
            slug: row.get("slug"),
            description: None,
            color: row.try_get("color").ok().flatten(),
            cover: row.try_get("cover").ok().flatten(),
        });
        upsert(index, &tag)?;
    }

    for row in sqlx::query("SELECT * FROM categories")
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?
    {
        let category = Category::new(row.get::<String, _>("name")).with_spec(CategorySpec {
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
        });
        upsert(index, &category)?;
    }

    for row in sqlx::query("SELECT * FROM comments")
        .fetch_all(pool)
        .await
        .map_err(map_sqlx)?
    {
        if row
            .try_get::<Option<String>, _>("parent_name")
            .ok()
            .flatten()
            .is_some()
        {
            let reply = Reply::new(row.get::<String, _>("name")).with_spec(ReplySpec {
                base: comment_base(&row),
                comment_name: row.get("parent_name"),
                quote_reply: row.try_get("quote_reply").ok().flatten(),
            });
            upsert(index, &reply)?;
        } else {
            let subject_kind: String = row.get("subject_kind");
            let subject_name: String = row.get("subject_name");
            let gvk = match subject_kind.as_str() {
                "SinglePage" => rblog_content::content::SinglePage::gvk(),
                _ => rblog_content::content::Post::gvk(),
            };
            let mut comment = Comment::new(row.get::<String, _>("name")).with_spec(CommentSpec {
                base: comment_base(&row),
                subject_ref: Ref::of_gvk(subject_name.clone(), &gvk),
                last_read_time: None,
            });
            comment.metadata.set_label(
                APPROVED_LABEL,
                if row.get::<i64, _>("approved") != 0 {
                    "true"
                } else {
                    "false"
                },
            );
            comment.metadata.set_label(SUBJECT_KIND_LABEL, subject_kind);
            comment.metadata.set_label(SUBJECT_NAME_LABEL, subject_name);
            upsert(index, &comment)?;
        }
    }
    Ok(())
}

fn comment_base(row: &sqlx::sqlite::SqliteRow) -> BaseCommentSpec {
    BaseCommentSpec {
        raw: row.get("raw"),
        content: row.get("html"),
        owner: CommentOwner {
            kind: row.try_get("owner_kind").ok().flatten().unwrap_or_default(),
            name: row.try_get("owner_name").ok().flatten().unwrap_or_default(),
            display_name: row.try_get("owner_display_name").ok().flatten(),
            annotations: None,
        },
        user_agent: row.try_get("user_agent").ok().flatten(),
        ip_address: row.try_get("ip_address").ok().flatten(),
        approved: row.get::<i64, _>("approved") != 0,
        approved_time: parse_dt(
            row.try_get::<Option<String>, _>("approved_at")
                .ok()
                .flatten(),
        ),
        creation_time: parse_dt(
            row.try_get::<Option<String>, _>("created_at")
                .ok()
                .flatten(),
        ),
        allow_notification: true,
        hidden: row.get::<i64, _>("hidden") != 0,
        priority: i32::try_from(row.get::<i64, _>("priority")).unwrap_or_default(),
        top: row.get::<i64, _>("top") != 0,
    }
}

fn parse_dt(raw: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    raw.and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
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
