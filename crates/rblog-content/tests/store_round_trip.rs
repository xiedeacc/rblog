//! End-to-end test: store typed kinds into the real SQLite-backed extension
//! store and read them back. Proves the `rblog-content` JSON shape and the
//! `rblog-store` TypedStore layer line up for real Halo workloads.

use rblog_content::{
    content::{Post, PostSpec, Snapshot, SnapshotSpec, Visible},
    core::{ConfigMap, User, UserSpec},
    infra::Ref,
    register_default_schemes,
};
use rblog_scheme::{Extension, SchemeRegistry};
use rblog_store::{run_migrations, AnyPool, TypedStore};

async fn fresh_pool() -> AnyPool {
    let pool = AnyPool::connect("sqlite::memory:")
        .await
        .expect("sqlite in-memory pool");
    run_migrations(&pool).await.expect("migrations apply");
    pool
}

#[tokio::test]
async fn post_round_trips_through_store() {
    let pool = fresh_pool().await;
    let store = TypedStore::new(&pool);

    let mut p = Post::new("hello").with_spec(PostSpec {
        title: "Hello, world!".to_owned(),
        slug: "hello".to_owned(),
        publish: true,
        visible: Visible::Public,
        owner: Some("admin".to_owned()),
        ..PostSpec::default()
    });
    p.metadata.set_label("content.halo.run/published", "true");

    let created: Post = store.create(&p).await.expect("create");
    let initial_version = created.metadata.version;
    assert!(
        initial_version.is_some(),
        "create must populate metadata.version"
    );

    let fetched: Post = store
        .fetch::<Post>(created.metadata.name())
        .await
        .expect("fetch")
        .expect("post must exist");
    assert_eq!(fetched.spec.as_ref().unwrap().title, "Hello, world!");
    assert_eq!(
        fetched.metadata.label("content.halo.run/published"),
        Some("true")
    );

    let all: Vec<Post> = store.list::<Post>().await.expect("list");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].metadata.name(), "hello");
}

#[tokio::test]
async fn snapshot_links_to_post_via_subject_ref() {
    let pool = fresh_pool().await;
    let store = TypedStore::new(&pool);

    let post = Post::new("a").with_spec(PostSpec {
        title: "A".to_owned(),
        slug: "a".to_owned(),
        ..PostSpec::default()
    });
    let _ = store.create(&post).await.expect("create post");

    let snap = Snapshot::new("snap-1").with_spec(SnapshotSpec {
        subject_ref: Ref::of_gvk("a", &Post::gvk()),
        raw_type: "markdown".to_owned(),
        raw_patch: Some("# A".to_owned()),
        content_patch: Some("<h1>A</h1>".to_owned()),
        parent_snapshot_name: None,
        last_modify_time: None,
        owner: "admin".to_owned(),
        contributors: None,
    });
    let _ = store.create(&snap).await.expect("create snap");

    let fetched: Snapshot = store
        .fetch::<Snapshot>(snap.metadata.name())
        .await
        .expect("fetch")
        .expect("snap must exist");
    let spec = fetched.spec.expect("spec");
    assert_eq!(spec.subject_ref.name, "a");
    assert_eq!(spec.subject_ref.kind, "Post");
    assert_eq!(spec.raw_patch.as_deref(), Some("# A"));
}

#[tokio::test]
async fn configmap_stores_flat_data() {
    let pool = fresh_pool().await;
    let store = TypedStore::new(&pool);

    let mut cm = ConfigMap::new("system");
    cm.put("site.title", "rblog");
    cm.put("site.subtitle", "a rust blog");
    let _ = store.create(&cm).await.expect("create");

    let fetched: ConfigMap = store
        .fetch::<ConfigMap>(cm.metadata.name())
        .await
        .expect("fetch")
        .expect("configmap must exist");
    let data = fetched.data.expect("data");
    assert_eq!(data.get("site.title").map(String::as_str), Some("rblog"));
}

#[tokio::test]
async fn user_credentials_round_trip_with_optimistic_lock() {
    let pool = fresh_pool().await;
    let store = TypedStore::new(&pool);

    let admin = User::new("admin").with_spec(UserSpec {
        display_name: "Admin".to_owned(),
        email: "admin@example.com".to_owned(),
        email_verified: true,
        password: Some("$argon2id$placeholder".to_owned()),
        ..UserSpec::default()
    });
    let mut created = store.create(&admin).await.expect("create");
    let v0 = created
        .metadata
        .version
        .expect("metadata.version after create");

    created.spec.as_mut().unwrap().display_name = "Site Admin".to_owned();
    let updated = store.update(&created).await.expect("update");
    let v1 = updated
        .metadata
        .version
        .expect("metadata.version after update");
    assert!(v1 > v0, "version must bump on update: {v0} -> {v1}");

    // Stale update using the old version must fail with Conflict.
    let stale = created;
    let err = store.update(&stale).await.expect_err("stale must conflict");
    assert!(
        matches!(err, rblog_store::StoreError::OptimisticLock { .. }),
        "expected OptimisticLock, got {err:?}"
    );
}

#[test]
fn registry_holds_every_default_kind() {
    let reg = SchemeRegistry::new();
    register_default_schemes(&reg).expect("register");

    // Every well-known Halo kind looks up to a GVK.
    let post = reg
        .lookup_by_group_plural("content.halo.run", "posts")
        .expect("posts gvk");
    assert_eq!(post.kind, "Post");

    let user = reg.lookup_by_group_plural("", "users").expect("users gvk");
    assert_eq!(user.kind, "User");

    let counter = reg
        .lookup_by_group_plural("metrics.halo.run", "counters")
        .expect("counters gvk");
    assert_eq!(counter.kind, "Counter");
}
