//! System integration helpers: index resync and the `Services` builder.

use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_content::content::{Category, Comment, Post, Reply, SinglePage, Snapshot, Tag};
use rblog_content::core::{ConfigMap, Menu, MenuItem, Role, RoleBinding, Setting, User};
use rblog_content::render::MarkdownPipeline;
use rblog_index::IndexEngine;
use rblog_store::AnyPool;

use crate::indexing::resync_kind;
use crate::{
    CategoryService, CommentService, ConfigMapService, MenuService, PageService, PostService,
    ServiceError, Services, SettingService, TagService, UserService,
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
    let menus = Arc::new(MenuService::new(pool.clone(), index.clone()));
    let settings = Arc::new(SettingService::new(pool.clone(), index.clone()));
    let configmaps = Arc::new(ConfigMapService::new(pool, index.clone()));
    Ok(Services {
        pages,
        posts,
        categories,
        tags,
        comments,
        users,
        menus,
        settings,
        configmaps,
        hasher,
        index,
    })
}

/// Resync every kind from the live store into `index`. Used at boot and
/// can be called from the admin API to force a refresh.
pub async fn resync_all(index: &Arc<IndexEngine>, pool: &AnyPool) -> Result<(), ServiceError> {
    resync_kind::<Post>(index, pool).await?;
    resync_kind::<SinglePage>(index, pool).await?;
    resync_kind::<Snapshot>(index, pool).await?;
    resync_kind::<Tag>(index, pool).await?;
    resync_kind::<Category>(index, pool).await?;
    resync_kind::<Comment>(index, pool).await?;
    resync_kind::<Reply>(index, pool).await?;
    resync_kind::<User>(index, pool).await?;
    resync_kind::<Role>(index, pool).await?;
    resync_kind::<RoleBinding>(index, pool).await?;
    resync_kind::<Menu>(index, pool).await?;
    resync_kind::<MenuItem>(index, pool).await?;
    resync_kind::<Setting>(index, pool).await?;
    resync_kind::<ConfigMap>(index, pool).await?;
    Ok(())
}
