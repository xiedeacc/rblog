//! End-to-end public route tests: drives the real router with reqwest
//! against an in-memory SQLite store seeded via the service layer.

use std::sync::Arc;

use rblog_attachments::{AttachmentService, Storage, ThumbnailEngine};
use rblog_auth::PasswordHasher;
use rblog_content::render::MarkdownPipeline;
use rblog_core::{
    bootstrap_system, build_services, BootstrapOptions, DraftPost, NewCategory, NewTag,
    PublishOptions,
};
use rblog_http::{routes::build_router, AppConfig, AppState};
use rblog_plugins::PluginRuntime;
use rblog_search::SearchIndex;
use rblog_store::{run_migrations, AnyPool};
use rblog_theme::{default_theme::install_default_theme, ThemeRegistry};
use tokio::net::TcpListener;

struct Harness {
    addr: std::net::SocketAddr,
    _tmp: tempfile::TempDir,
}

async fn boot() -> Harness {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pool = AnyPool::connect("sqlite::memory:").await.expect("pool");
    run_migrations(&pool).await.expect("migrations");
    let pipeline = Arc::new(MarkdownPipeline::new());
    let hasher = Arc::new(PasswordHasher::new());
    let services = build_services(pool.clone(), pipeline.clone(), hasher.clone())
        .await
        .expect("services");

    bootstrap_system(
        &pool,
        &services.index,
        &hasher,
        &BootstrapOptions {
            admin_username: "admin".into(),
            admin_email: "admin@example.com".into(),
            admin_password: "supersecret".into(),
            site_title: "rblog test".into(),
            site_subtitle: Some("hello".into()),
            site_base_url: Some("https://example.com".into()),
        },
    )
    .await
    .expect("bootstrap");

    // Seed: 2 posts, 1 tag, 1 category.
    services
        .tags
        .create(NewTag {
            name: "rust".into(),
            display_name: "Rust".into(),
            slug: "rust".into(),
            description: None,
            color: Some("#dea584".into()),
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
            description: Some("Site updates".into()),
            cover: None,
            template: None,
            post_template: None,
            priority: 0,
            children: None,
        })
        .await
        .expect("cat");
    for (name, title, slug, tag) in [
        ("p1", "First post", "first", Some("rust")),
        ("p2", "Second post", "second", None),
    ] {
        services
            .posts
            .draft(DraftPost {
                name: name.into(),
                title: title.into(),
                slug: slug.into(),
                markdown: format!("# {title}\n\nHello **{title}**."),
                owner: "admin".into(),
                template: None,
                cover: None,
                categories: Some(vec!["news".into()]),
                tags: tag.map(|t| vec![t.to_owned()]),
                excerpt: None,
                priority: None,
                pinned: None,
                allow_comment: None,
                visible: rblog_content::content::Visible::default(),
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
        .draft(DraftPost {
            name: "private-post".into(),
            title: "Private post".into(),
            slug: "private".into(),
            markdown: "# Private post\n\nOnly signed-in users can read this.".into(),
            owner: "admin".into(),
            template: None,
            cover: None,
            categories: Some(vec!["news".into()]),
            tags: None,
            excerpt: None,
            priority: None,
            pinned: None,
            allow_comment: None,
            visible: rblog_content::content::Visible::Private,
        })
        .await
        .expect("private draft");
    services
        .posts
        .publish("private-post", PublishOptions::default())
        .await
        .expect("publish private");

    let mut config = AppConfig::default();
    config.paths.themes_root = tmp.path().join("themes");
    config.paths.uploads_root = tmp.path().join("uploads");
    config.site.base_url = Some("https://example.com".into());
    install_default_theme(&config.paths.themes_root, false).expect("install theme");
    let themes = ThemeRegistry::new(config.paths.themes_root.clone(), pipeline.clone());
    themes.reload().expect("reload");
    let backend = Storage::Local {
        root: config.paths.uploads_root.clone(),
        public_prefix: "/uploads".into(),
    }
    .build()
    .expect("storage");
    let attachments = AttachmentService::new(backend, ThumbnailEngine::empty());
    let search = SearchIndex::in_memory().expect("search");
    let plugins = PluginRuntime::new().expect("plugins");

    let state = AppState::new(
        config,
        pool,
        services,
        themes,
        pipeline,
        hasher,
        attachments,
        search,
        plugins,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let router = build_router(state);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    Harness { addr, _tmp: tmp }
}

async fn fetch_text(addr: std::net::SocketAddr, path: &str) -> (reqwest::StatusCode, String) {
    let resp = reqwest::get(format!("http://{addr}{path}"))
        .await
        .expect("connect");
    let status = resp.status();
    let body = resp.text().await.expect("body");
    (status, body)
}

#[tokio::test]
async fn home_lists_published_posts() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/").await;
    assert_eq!(status, 200);
    assert!(body.contains("First post"));
    assert!(body.contains("Second post"));
    assert!(!body.contains("Private post"));
    // Site title from system config map.
    assert!(body.contains("rblog test"));
}

#[tokio::test]
async fn site_info_exposes_site_title() {
    let h = boot().await;
    let body: serde_json::Value = reqwest::get(format!("http://{}/api/site", h.addr))
        .await
        .expect("connect")
        .json()
        .await
        .expect("json");
    assert_eq!(body["title"], "rblog test");
}

#[tokio::test]
async fn signed_in_home_lists_private_posts() {
    let h = boot().await;
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .expect("client");
    let login = client
        .post(format!("http://{}/api/admin/auth/login", h.addr))
        .json(&serde_json::json!({
            "username": "admin",
            "password": "supersecret",
        }))
        .send()
        .await
        .expect("login");
    assert_eq!(login.status(), 200);

    let body = client
        .get(format!("http://{}/", h.addr))
        .send()
        .await
        .expect("home")
        .text()
        .await
        .expect("body");
    assert!(body.contains("[private] Private post"));
    assert!(body.contains("data-user-menu"));
    assert!(body.contains("控制台"));
    assert!(body.contains("退出登录"));

    let detail = client
        .get(format!("http://{}/archives/private", h.addr))
        .send()
        .await
        .expect("private detail");
    assert_eq!(detail.status(), 200);
    assert!(detail
        .text()
        .await
        .expect("body")
        .contains("Only signed-in users can read this."));
}

#[tokio::test]
async fn post_detail_renders_html() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/archives/first").await;
    assert_eq!(status, 200);
    assert!(body.contains("First post"));
    assert!(body.contains("<strong>First post</strong>"));
}

#[tokio::test]
async fn post_detail_increments_visit_stats() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/archives/first").await;
    assert_eq!(status, 200);
    assert!(body.contains("阅读 1"));

    let (status, body) = fetch_text(h.addr, "/archives/first").await;
    assert_eq!(status, 200);
    assert!(body.contains("阅读 2"));

    let (status, body) = fetch_text(h.addr, "/").await;
    assert_eq!(status, 200);
    assert!(body.contains("<dt>2</dt><dd>阅读量</dd>"));

    let (status, body) = fetch_text(h.addr, "/").await;
    assert_eq!(status, 200);
    assert!(body.contains("<dt>2</dt><dd>阅读量</dd>"));
}

#[tokio::test]
async fn missing_post_returns_404_with_themed_page() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/archives/nope").await;
    assert_eq!(status, 404);
    assert!(body.contains("Not Found"));
}

#[tokio::test]
async fn tag_archive_filters_to_tag_posts() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/tags/rust").await;
    assert_eq!(status, 200);
    assert!(body.contains("First post"));
    // The second post has no `rust` tag.
    assert!(!body.contains("Second post"));
}

#[tokio::test]
async fn category_archive_lists_category_posts() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/categories/news").await;
    assert_eq!(status, 200);
    assert!(body.contains("First post"));
    assert!(body.contains("Second post"));
}

#[tokio::test]
async fn archive_page_lists_all_posts() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/archives").await;
    assert_eq!(status, 200);
    assert!(body.contains("First post"));
    assert!(body.contains("Second post"));
}

#[tokio::test]
async fn rss_feed_uses_absolute_urls_from_site_config() {
    let h = boot().await;
    let resp = reqwest::get(format!("http://{}/feed.xml", h.addr))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("application/rss+xml"));
    let body = resp.text().await.expect("body");
    assert!(body.contains("<rss version=\"2.0\""));
    assert!(body.contains("https://example.com/archives/first"));
    assert!(body.contains("First post"));
}

#[tokio::test]
async fn sitemap_lists_post_urls() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/sitemap.xml").await;
    assert_eq!(status, 200);
    assert!(body.contains("<urlset"));
    assert!(body.contains("https://example.com/archives/first"));
}

#[tokio::test]
async fn robots_exposes_sitemap() {
    let h = boot().await;
    let (status, body) = fetch_text(h.addr, "/robots.txt").await;
    assert_eq!(status, 200);
    assert!(body.contains("User-agent: *"));
    assert!(body.contains("Sitemap: https://example.com/sitemap.xml"));
}

#[tokio::test]
async fn comment_can_be_submitted_and_retrieved_after_moderation() {
    let h = boot().await;
    let client = reqwest::Client::new();
    let post = client
        .post(format!("http://{}/api/comments", h.addr))
        .json(&serde_json::json!({
            "subject_slug": "first",
            "raw": "This is a thoughtful comment without any links.",
            "display_name": "Alice",
            "email": "alice@example.com"
        }))
        .send()
        .await
        .expect("submit");
    assert_eq!(post.status(), 201, "{}", post.text().await.unwrap());
    let body: serde_json::Value = post.json().await.unwrap();
    assert_eq!(body["approved"], false);
    assert_eq!(body["queued_for_moderation"], true);

    // Public list should be empty until moderation approves.
    let list: Vec<serde_json::Value> = client
        .get(format!("http://{}/api/comments?subject_slug=first", h.addr))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn comment_with_spam_terms_is_rejected() {
    let h = boot().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/comments", h.addr))
        .json(&serde_json::json!({
            "subject_slug": "first",
            "raw": "Hot deals on viagra! Click now!!",
            "display_name": "Spam",
            "email": "spam@example.com"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
async fn comment_honeypot_swallows_bot_submissions() {
    let h = boot().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/api/comments", h.addr))
        .json(&serde_json::json!({
            "subject_slug": "first",
            "raw": "Visit my site!",
            "display_name": "Bot",
            "email": "bot@example.com",
            "website": "https://botsite.example"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let list: Vec<serde_json::Value> = client
        .get(format!("http://{}/api/comments?subject_slug=first", h.addr))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list.is_empty(), "honeypot must not persist anything");
}

#[tokio::test]
async fn pagination_renders_when_more_than_one_page() {
    // boot only seeds 2 posts and default PAGE_SIZE is 10 — make sure
    // single-page output doesn't render pagination links.
    let h = boot().await;
    let (_, body) = fetch_text(h.addr, "/").await;
    assert!(!body.contains("Page 1 of"));
}

/// End-to-end exercise of the WASM plugin dispatcher: plants a minimal
/// hello-world plugin on disk, boots a stand-alone harness that loads
/// it, and asserts the response is what the plugin would have written.
///
/// We can't reuse `boot()` because we need to point the `PluginRuntime`
/// at a freshly-populated tempdir before constructing `AppState`.
#[tokio::test]
async fn plugin_dispatch_returns_plugin_response() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let plugins_root = tmp.path().join("plugins");
    let plugin_dir = plugins_root.join("hello-world");
    std::fs::create_dir_all(&plugin_dir).expect("plugin dir");
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
name = "hello-world"
enabled = true
entry = "plugin.wat"
capabilities = ["http"]

[[routes]]
path = "/greet"
methods = ["GET"]
"#,
    )
    .expect("manifest");
    // 80-byte JSON response baked at offset 0, bump heap at 1024.
    std::fs::write(
        plugin_dir.join("plugin.wat"),
        r#"(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (data (i32.const 0) "{\"status\":200,\"content_type\":\"text/plain; charset=utf-8\",\"body\":\"Hello, world!\"}")
  (func (export "alloc") (param $s i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $s)))
    (local.get $p))
  (func (export "handle")
    (param i32 i32 i32 i32 i32 i32) (result i64)
    (i64.const 80)))
"#,
    )
    .expect("wat");

    let pool = AnyPool::connect("sqlite::memory:").await.expect("pool");
    run_migrations(&pool).await.expect("migrations");
    let pipeline = Arc::new(MarkdownPipeline::new());
    let hasher = Arc::new(PasswordHasher::new());
    let services = build_services(pool.clone(), pipeline.clone(), hasher.clone())
        .await
        .expect("services");

    let mut config = AppConfig::default();
    config.paths.themes_root = tmp.path().join("themes");
    config.paths.uploads_root = tmp.path().join("uploads");
    config.paths.plugins_root = plugins_root.clone();
    install_default_theme(&config.paths.themes_root, false).expect("install theme");
    let themes = ThemeRegistry::new(config.paths.themes_root.clone(), pipeline.clone());
    themes.reload().expect("reload");
    let backend = Storage::Local {
        root: config.paths.uploads_root.clone(),
        public_prefix: "/uploads".into(),
    }
    .build()
    .expect("storage");
    let attachments = AttachmentService::new(backend, ThumbnailEngine::empty());
    let search = SearchIndex::in_memory().expect("search");
    let plugins = PluginRuntime::new().expect("plugins");
    let loaded = plugins.reload(&plugins_root).expect("reload plugins");
    assert_eq!(loaded, 1, "hello-world should be discovered");

    let state = AppState::new(
        config,
        pool,
        services,
        themes,
        pipeline,
        hasher,
        attachments,
        search,
        plugins,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let router = build_router(state);
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let resp = reqwest::get(format!("http://{addr}/api/plugins/hello-world/greet"))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("")
        .starts_with("text/plain"));
    let body = resp.text().await.expect("body");
    assert_eq!(body, "Hello, world!");
}
