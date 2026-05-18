//! End-to-end tests for the core service layer.

use std::sync::Arc;

use rblog_auth::PasswordHasher;
use rblog_content::content::{CommentOwner, Visible};
use rblog_content::render::MarkdownPipeline;
use rblog_core::{
    bootstrap_system, build_services, BootstrapOptions, CreateUser, DraftPost, NewCategory,
    NewComment, NewTag, PostListQuery, PostStatusFilter, PublishOptions,
};
use rblog_store::{run_migrations, AnyPool};

async fn setup() -> rblog_core::Services {
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
    services
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
        .expect("list");
    assert_eq!(listed.total, 1);

    // Empty default published filter -> no posts yet.
    let published_only = services.posts.list(PostListQuery::default()).expect("list");
    assert_eq!(published_only.total, 0);

    services
        .posts
        .publish("first", PublishOptions::default())
        .await
        .expect("publish");

    let visible = services.posts.list(PostListQuery::default()).expect("list");
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

    let tag_stats = services.tags.stats().expect("tag stats");
    assert_eq!(tag_stats.len(), 1);
    assert_eq!(tag_stats[0].post_count, 1);

    let cat_stats = services.categories.stats().expect("cat stats");
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
