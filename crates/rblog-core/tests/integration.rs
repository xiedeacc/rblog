//! End-to-end tests for the core service layer.

use std::collections::BTreeSet;
use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_content::content::{CommentOwner, Visible};
use rblog_content::render::MarkdownPipeline;
use rblog_core::{
    bootstrap_system, build_services, BootstrapOptions, CreateUser, DraftPost, NewCategory,
    NewComment, NewTag, PostListQuery, PostSettingsUpdate, PostStatusFilter, PublishOptions,
};
use rblog_store::{run_migrations, AnyPool};
use sqlx::Row;

async fn setup() -> rblog_core::Services {
    setup_with_pool().await.0
}

async fn setup_with_pool() -> (rblog_core::Services, AnyPool) {
    let pool = AnyPool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    run_migrations(&pool).await.expect("migrations");
    let pipeline = Arc::new(MarkdownPipeline::new());
    let hasher = Arc::new(PasswordHasher::new());
    let services = build_services(pool.clone(), pipeline, hasher.clone())
        .await
        .expect("build services");
    let opts = BootstrapOptions {
        admin_username: "admin".into(),
        admin_email: "admin@example.com".into(),
        admin_password: "supersecret".into(),
        site_title: "rblog tests".into(),
        site_subtitle: Some("hello".into()),
        site_base_url: Some("https://example.com".into()),
    };
    bootstrap_system(&pool, &services.index, &hasher, &opts)
        .await
        .expect("bootstrap");
    (services, pool)
}

#[tokio::test]
async fn bootstrap_is_idempotent() {
    let services = setup().await;
    let admin = services.users.get("admin").await.expect("admin loaded");
    assert_eq!(admin.metadata.name, "admin");
    assert!(admin
        .spec
        .unwrap()
        .password
        .unwrap()
        .starts_with("$argon2id$"));
    let cm = services.configmaps.system().await.expect("system cm");
    let data = cm.data.unwrap();
    assert_eq!(
        data.get("site.title").map(String::as_str),
        Some("rblog tests")
    );
    assert_eq!(
        data.get("site.baseUrl").map(String::as_str),
        Some("https://example.com")
    );
}

#[tokio::test]
async fn draft_publish_and_list_post() {
    let services = setup().await;
    let draft = services
        .posts
        .draft(DraftPost {
            name: "first".into(),
            title: "First Post".into(),
            slug: "first".into(),
            markdown: "# Hello\n\nWelcome to **rblog**.".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: Some(vec!["news".into()]),
            tags: Some(vec!["rust".into()]),
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("draft");
    assert_eq!(draft.title, "First Post");
    assert!(draft.content_html.contains("<strong>rblog</strong>"));
    assert!(!draft.published);

    let listed = services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Any,
            ..PostListQuery::default()
        })
        .await
        .expect("list");
    assert_eq!(listed.total, 1);

    // Empty default published filter -> no posts yet.
    let published_only = services
        .posts
        .list(PostListQuery::default())
        .await
        .expect("list");
    assert_eq!(published_only.total, 0);

    services
        .posts
        .publish("first", PublishOptions::default())
        .await
        .expect("publish");

    let visible = services
        .posts
        .list(PostListQuery::default())
        .await
        .expect("list");
    assert_eq!(visible.total, 1);
    assert_eq!(visible.items[0].slug, "first");
    assert_eq!(visible.items[0].permalink, "/archives/first");

    let detail = services
        .posts
        .public_by_slug("first")
        .await
        .expect("by slug");
    assert!(detail.published);
    assert_eq!(detail.tags, vec!["rust".to_owned()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_post_visit_increments_are_exact() {
    let services = setup().await;
    services
        .posts
        .draft(DraftPost {
            name: "popular".into(),
            title: "Popular Post".into(),
            slug: "popular".into(),
            markdown: "# Popular\n\nLots of readers.".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: None,
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("draft");
    services
        .posts
        .publish("popular", PublishOptions::default())
        .await
        .expect("publish");

    let mut tasks = Vec::new();
    for _ in 0..64 {
        let posts = services.posts.clone();
        tasks.push(tokio::spawn(async move {
            posts.increment_visit("popular").await.expect("increment")
        }));
    }

    let mut returned = BTreeSet::new();
    for task in tasks {
        returned.insert(task.await.expect("join"));
    }

    assert_eq!(
        returned.len(),
        64,
        "each caller should see a distinct count"
    );
    assert_eq!(returned.first().copied(), Some(1));
    assert_eq!(returned.last().copied(), Some(64));
    let detail = services
        .posts
        .admin_detail("popular")
        .await
        .expect("detail");
    assert_eq!(detail.visits, 64);
}

#[tokio::test]
async fn post_slug_must_be_unique() {
    let services = setup().await;
    services
        .posts
        .draft(DraftPost {
            name: "first".into(),
            title: "First".into(),
            slug: "same-slug".into(),
            markdown: "first".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: None,
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("first draft");

    let duplicate = services
        .posts
        .draft(DraftPost {
            name: "second".into(),
            title: "Second".into(),
            slug: "same-slug".into(),
            markdown: "second".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: None,
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await;
    assert!(duplicate.is_err());

    let update = services
        .posts
        .update_settings(
            "first",
            PostSettingsUpdate {
                slug: Some("same-slug".into()),
                ..PostSettingsUpdate::default()
            },
        )
        .await
        .expect("same post can keep slug");
    assert_eq!(update.slug, "same-slug");
}

#[tokio::test]
async fn update_post_content_rerenders() {
    let services = setup().await;
    services
        .posts
        .draft(DraftPost {
            name: "p".into(),
            title: "P".into(),
            slug: "p".into(),
            markdown: "first".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: None,
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("draft");
    let updated = services
        .posts
        .update_content("p", "second", "admin")
        .await
        .expect("update");
    assert!(updated.content_html.contains("second"));
}

#[tokio::test]
async fn pinned_posts_sort_before_regular_posts() {
    let services = setup().await;
    for name in ["regular", "pinned"] {
        services
            .posts
            .draft(DraftPost {
                name: name.into(),
                title: name.into(),
                slug: name.into(),
                markdown: name.into(),
                owner: "admin".into(),
                template: None,
                cover: None,
                categories: None,
                tags: None,
                excerpt: None,
                priority: None,
                pinned: None,
                allow_comment: None,
                visible: Visible::default(),
            })
            .await
            .expect("draft");
        services
            .posts
            .publish(name, PublishOptions::default())
            .await
            .expect("publish");
    }
    services
        .posts
        .update_settings(
            "pinned",
            PostSettingsUpdate {
                pinned: Some(true),
                ..PostSettingsUpdate::default()
            },
        )
        .await
        .expect("pin");

    let listed = services
        .posts
        .list(PostListQuery::default())
        .await
        .expect("list");

    assert_eq!(listed.items[0].name, "pinned");
    assert!(listed.items[0].pinned);
}

#[tokio::test]
async fn drafts_sort_after_pinned_and_have_time() {
    let services = setup().await;
    for name in ["published", "draft", "pinned"] {
        services
            .posts
            .draft(DraftPost {
                name: name.into(),
                title: name.into(),
                slug: name.into(),
                markdown: name.into(),
                owner: "admin".into(),
                template: None,
                cover: None,
                categories: None,
                tags: None,
                excerpt: None,
                priority: None,
                pinned: None,
                allow_comment: None,
                visible: Visible::default(),
            })
            .await
            .expect("draft");
    }
    services
        .posts
        .publish("published", PublishOptions::default())
        .await
        .expect("publish");
    services
        .posts
        .publish("pinned", PublishOptions::default())
        .await
        .expect("publish pinned");
    services
        .posts
        .update_settings(
            "pinned",
            PostSettingsUpdate {
                pinned: Some(true),
                ..PostSettingsUpdate::default()
            },
        )
        .await
        .expect("pin");

    let listed = services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Any,
            ..PostListQuery::default()
        })
        .await
        .expect("list");

    assert_eq!(listed.items[0].name, "pinned");
    assert_eq!(listed.items[1].name, "draft");
    assert!(listed.items[1].last_modify_time.is_some());
}

#[tokio::test]
async fn purge_removes_only_target_post_snapshots() {
    let (services, pool) = setup_with_pool().await;
    for name in ["target", "other"] {
        services
            .posts
            .draft(DraftPost {
                name: name.into(),
                title: name.into(),
                slug: name.into(),
                markdown: name.into(),
                owner: "admin".into(),
                template: None,
                cover: None,
                categories: None,
                tags: None,
                excerpt: None,
                priority: None,
                pinned: None,
                allow_comment: None,
                visible: Visible::default(),
            })
            .await
            .expect("draft");
    }

    services.posts.purge("target").await.expect("purge");
    let sqlite = match &pool {
        AnyPool::Sqlite(pool) => pool,
        AnyPool::Mysql(_) => unreachable!(),
    };
    let target = sqlx::query("SELECT COUNT(*) AS count FROM posts WHERE name = 'target'")
        .fetch_one(sqlite)
        .await
        .unwrap()
        .get::<i64, _>("count");
    let other = sqlx::query("SELECT COUNT(*) AS count FROM posts WHERE name = 'other'")
        .fetch_one(sqlite)
        .await
        .unwrap()
        .get::<i64, _>("count");
    assert_eq!(target, 0);
    assert_eq!(other, 1);
}

#[tokio::test]
async fn admin_detail_handles_imported_post_without_snapshots() {
    let (services, pool) = setup_with_pool().await;
    let sqlite = match &pool {
        AnyPool::Sqlite(pool) => pool,
        AnyPool::Mysql(_) => unreachable!(),
    };
    sqlx::query(
        "INSERT INTO posts (name, title, slug, markdown, html, raw_type, published, visible, deleted, pinned, allow_comment, priority, visits) VALUES ('legacy-empty', 'Legacy Empty', 'legacy-empty', '', '', 'markdown', 0, 'PUBLIC', 0, 0, 1, 0, 0)",
    )
    .execute(sqlite)
    .await
    .expect("create imported post");

    let detail = services
        .posts
        .admin_detail("legacy-empty")
        .await
        .expect("admin detail");

    assert_eq!(detail.title, "Legacy Empty");
    assert_eq!(detail.raw_markdown, "");
    assert_eq!(detail.content_html, "");
    assert_eq!(detail.raw_type, "markdown");

    let updated = services
        .posts
        .update_content("legacy-empty", "new body", "admin")
        .await
        .expect("update content");
    assert_eq!(updated.raw_markdown, "new body");
    assert!(updated.content_html.contains("new body"));
}

#[tokio::test]
async fn tag_and_category_stats_count_published_posts() {
    let services = setup().await;
    services
        .tags
        .create(NewTag {
            name: "rust".into(),
            display_name: "Rust".into(),
            slug: "rust".into(),
            description: None,
            color: None,
            cover: None,
        })
        .await
        .expect("tag");
    services
        .categories
        .create(NewCategory {
            name: "news".into(),
            display_name: "News".into(),
            slug: "news".into(),
            description: None,
            cover: None,
            template: None,
            post_template: None,
            priority: 0,
            children: None,
        })
        .await
        .expect("cat");
    services
        .posts
        .draft(DraftPost {
            name: "p".into(),
            title: "P".into(),
            slug: "p".into(),
            markdown: "hi".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: Some(vec!["news".into()]),
            tags: Some(vec!["rust".into()]),
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("draft");
    services
        .posts
        .publish("p", PublishOptions::default())
        .await
        .expect("publish");

    let tag_stats = services.tags.stats().await.expect("tag stats");
    assert_eq!(tag_stats.len(), 1);
    assert_eq!(tag_stats[0].post_count, 1);

    let cat_stats = services.categories.stats().await.expect("cat stats");
    assert_eq!(cat_stats.len(), 1);
    assert_eq!(cat_stats[0].post_count, 1);
}

#[tokio::test]
async fn comment_moderation_queue_holds_anonymous_until_approved() {
    let services = setup().await;
    services
        .posts
        .draft(DraftPost {
            name: "x".into(),
            title: "X".into(),
            slug: "x".into(),
            markdown: "x".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: None,
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: Visible::default(),
        })
        .await
        .expect("draft");
    services
        .posts
        .publish("x", PublishOptions::default())
        .await
        .expect("publish");

    let owner = CommentOwner {
        kind: "Email".into(),
        name: "anon@example.com".into(),
        display_name: Some("Anon".into()),
        annotations: None,
    };
    let c = services
        .comments
        .submit(NewComment {
            subject_kind: Some("Post".into()),
            subject_name: "x".into(),
            raw: "Nice post!".into(),
            owner,
            user_agent: None,
            ip_address: None,
            quote_reply: None,
        })
        .await
        .expect("submit");

    let queue = services.comments.moderation_queue().expect("queue");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].metadata.name, c.metadata.name);
    let public = services
        .comments
        .public_thread("Post", "x")
        .expect("thread");
    assert!(public.is_empty(), "anon comments are not auto-published");

    services
        .comments
        .approve(&c.metadata.name)
        .await
        .expect("approve");
    let public = services
        .comments
        .public_thread("Post", "x")
        .expect("thread");
    assert_eq!(public.len(), 1);
}

#[tokio::test]
async fn user_authenticate_succeeds_for_correct_password() {
    let services = setup().await;
    services
        .users
        .create(CreateUser {
            name: "alice".into(),
            display_name: "Alice".into(),
            email: "alice@example.com".into(),
            password: "longenoughpw".into(),
        })
        .await
        .expect("create");
    let ok = services
        .users
        .authenticate("alice", "longenoughpw")
        .await
        .expect("login");
    assert_eq!(ok.email, "alice@example.com");

    let err = services
        .users
        .authenticate("alice", "wrong")
        .await
        .expect_err("must fail");
    assert!(matches!(err, rblog_core::ServiceError::Auth(_)));
}
