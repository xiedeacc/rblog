//! End-to-end: bind a real TCP listener, hit it with reqwest.

use std::sync::Arc;

use rblog_attachments::{AttachmentService, Storage, ThumbnailEngine};
use rblog_auth::PasswordHasher;
use rblog_content::render::MarkdownPipeline;
use rblog_core::build_services;
use rblog_http::{routes::build_router, AppConfig, AppState};
use rblog_plugins::PluginRuntime;
use rblog_search::SearchIndex;
use rblog_store::{run_migrations, AnyPool};
use rblog_theme::{default_theme::install_default_theme, ThemeRegistry};
use reqwest::header;
use tokio::net::TcpListener;

async fn boot() -> (AppState, std::net::SocketAddr) {
    let tmp = tempfile::tempdir().expect("tempdir");
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
    install_default_theme(&config.paths.themes_root, false).expect("install theme");
    let themes = ThemeRegistry::new(config.paths.themes_root.clone(), pipeline.clone());
    themes.reload().expect("reload themes");
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
    // Bind on port 0 then read back the actual port.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let router = build_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    // Give the server a tick to start accepting.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::mem::forget(tmp); // keep the themes dir alive for the test
    (state, addr)
}

#[tokio::test]
async fn liveness_returns_ok_json() {
    let (_state, addr) = boot().await;
    let resp = reqwest::get(format!("http://{addr}/api/health"))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn readiness_reports_backend_and_counts() {
    let (_state, addr) = boot().await;
    let resp = reqwest::get(format!("http://{addr}/api/health/ready"))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("json");
    assert_eq!(body["state"], "ready");
    assert_eq!(body["backend"], "sqlite");
    assert!(body["posts"].is_u64());
    assert!(body["users"].is_u64());
}

#[tokio::test]
async fn request_id_is_round_tripped() {
    let (_state, addr) = boot().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/api/health"))
        .send()
        .await
        .expect("connect");
    let id = resp
        .headers()
        .get("x-request-id")
        .expect("x-request-id header")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(!id.is_empty());
}

#[tokio::test]
async fn security_headers_are_set() {
    let (_state, addr) = boot().await;
    let resp = reqwest::get(format!("http://{addr}/api/health"))
        .await
        .expect("connect");
    let h = resp.headers();
    assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
    assert_eq!(h.get("x-frame-options").unwrap(), "SAMEORIGIN");
}

#[tokio::test]
async fn admin_spa_stub_responds_on_admin_path() {
    let (_state, addr) = boot().await;
    let resp = reqwest::get(format!("http://{addr}/admin"))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    let ct = resp
        .headers()
        .get("content-type")
        .expect("content-type")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(ct.starts_with("text/html"));
    let body = resp.text().await.expect("body");
    assert!(body.contains("rblog admin SPA"));
}

#[tokio::test]
async fn admin_spa_unknown_path_falls_back_to_html() {
    let (_state, addr) = boot().await;
    let resp = reqwest::get(format!("http://{addr}/admin/posts/edit/42"))
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("body");
    assert!(body.contains("rblog admin SPA"));
}

#[tokio::test]
async fn browser_first_visit_redirects_to_admin_bootstrap_until_setup() {
    let (_state, addr) = boot().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");

    let resp = client
        .get(format!("http://{addr}/"))
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .expect("connect");
    assert_eq!(resp.status(), 307);
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "/admin/bootstrap"
    );

    let bootstrap = client
        .post(format!("http://{addr}/api/admin/bootstrap"))
        .json(&serde_json::json!({
            "admin_username": "admin",
            "admin_password": "supersecret"
        }))
        .send()
        .await
        .expect("bootstrap");
    assert_eq!(bootstrap.status(), 200);

    let resp = client
        .get(format!("http://{addr}/"))
        .header(header::ACCEPT, "text/html")
        .send()
        .await
        .expect("connect");
    assert_eq!(resp.status(), 200);
}
