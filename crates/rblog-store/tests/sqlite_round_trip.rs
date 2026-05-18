//! End-to-end tests against an in-memory SQLite database.
//!
//! These tests exercise the entire storage primitive: raw CRUD + typed
//! (de)serialization + optimistic concurrency conflict semantics.
//! MySQL is not exercised here because it would require a live server; a
//! companion test using `testcontainers` will land separately.

use std::collections::BTreeMap;

use rblog_scheme::{Extension, GroupVersionKind, Metadata};
use rblog_store::raw::{AnyPool, RawStore, StoreError};
use rblog_store::TypedStore;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Fixture: a minimal Halo-shaped Extension we use to drive the store layer.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToyPostSpec {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToyPost {
    api_version: String,
    kind: String,
    metadata: Metadata,
    spec: ToyPostSpec,
}

impl ToyPost {
    fn new(name: &str, title: &str) -> Self {
        let gvk = Self::gvk();
        Self {
            api_version: gvk.api_version(),
            kind: gvk.kind.to_owned(),
            metadata: Metadata::new(name),
            spec: ToyPostSpec {
                title: title.to_owned(),
                slug: None,
            },
        }
    }
}

impl Extension for ToyPost {
    fn gvk() -> GroupVersionKind {
        GroupVersionKind::new("content.halo.run", "v1alpha1", "Post", "posts", "post")
    }
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    fn metadata_mut(&mut self) -> &mut Metadata {
        &mut self.metadata
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

async fn pool() -> AnyPool {
    let pool = AnyPool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool");
    rblog_store::run_migrations(&pool)
        .await
        .expect("migrations");
    pool
}

// ---------------------------------------------------------------------------
// Raw layer tests.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_then_fetch_raw() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/hello";
    let data = br#"{"apiVersion":"content.halo.run/v1alpha1","kind":"Post","metadata":{"name":"hello"},"spec":{"title":"Hi"}}"#;
    let row = pool.create(name, data).await.unwrap();
    assert_eq!(row.name, name);
    assert_eq!(row.version, 1);
    assert_eq!(row.data, data);

    let fetched = pool.fetch(name).await.unwrap().unwrap();
    assert_eq!(fetched, row);

    let absent = pool
        .fetch("/registry/content.halo.run/posts/none")
        .await
        .unwrap();
    assert!(absent.is_none());
}

#[tokio::test]
async fn duplicate_name_errors() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/dup";
    pool.create(name, b"{}").await.unwrap();
    let err = pool.create(name, b"{}").await.unwrap_err();
    assert!(matches!(err, StoreError::DuplicateName(_)));
}

#[tokio::test]
async fn update_increments_version() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/v";
    pool.create(name, b"{\"v\":1}").await.unwrap();
    let row = pool.update(name, 1, b"{\"v\":2}").await.unwrap();
    assert_eq!(row.version, 2);
    let row = pool.update(name, 2, b"{\"v\":3}").await.unwrap();
    assert_eq!(row.version, 3);
}

#[tokio::test]
async fn update_with_wrong_version_conflicts() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/conf";
    pool.create(name, b"{}").await.unwrap();
    let err = pool.update(name, 42, b"{}").await.unwrap_err();
    assert!(matches!(err, StoreError::OptimisticLock { .. }));
}

#[tokio::test]
async fn delete_with_correct_version_works() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/del";
    pool.create(name, b"{}").await.unwrap();
    let prev = pool.delete(name, 1).await.unwrap();
    assert_eq!(prev.version, 1);
    assert!(pool.fetch(name).await.unwrap().is_none());
}

#[tokio::test]
async fn delete_with_wrong_version_conflicts() {
    let pool = pool().await;
    let name = "/registry/content.halo.run/posts/del2";
    pool.create(name, b"{}").await.unwrap();
    let err = pool.delete(name, 99).await.unwrap_err();
    assert!(matches!(err, StoreError::OptimisticLock { .. }));
    assert!(
        pool.fetch(name).await.unwrap().is_some(),
        "row must not be gone"
    );
}

#[tokio::test]
async fn delete_missing_is_not_found() {
    let pool = pool().await;
    let err = pool
        .delete("/registry/content.halo.run/posts/ghost", 1)
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::NotFound(_)));
}

#[tokio::test]
async fn list_by_prefix_is_lexicographic() {
    let pool = pool().await;
    pool.create("/registry/content.halo.run/posts/b", b"{}")
        .await
        .unwrap();
    pool.create("/registry/content.halo.run/posts/a", b"{}")
        .await
        .unwrap();
    pool.create("/registry/content.halo.run/posts/c", b"{}")
        .await
        .unwrap();
    // A row of a different kind must not be returned.
    pool.create("/registry/content.halo.run/tags/x", b"{}")
        .await
        .unwrap();

    let listed = pool
        .list_by_prefix("/registry/content.halo.run/posts")
        .await
        .unwrap();
    let names: Vec<_> = listed.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "/registry/content.halo.run/posts/a",
            "/registry/content.halo.run/posts/b",
            "/registry/content.halo.run/posts/c",
        ]
    );
}

#[tokio::test]
async fn prefix_does_not_overmatch_sibling_kinds() {
    // Critical: a prefix `/registry/users` must not match `/registry/usersettings/...`.
    let pool = pool().await;
    pool.create("/registry/users/admin", b"{}").await.unwrap();
    pool.create("/registry/usersettings/admin", b"{}")
        .await
        .unwrap();
    let listed = pool.list_by_prefix("/registry/users").await.unwrap();
    let names: Vec<_> = listed.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["/registry/users/admin"]);
}

#[tokio::test]
async fn paged_listing_uses_cursor() {
    let pool = pool().await;
    for name in ["a", "b", "c", "d", "e"] {
        pool.create(&format!("/registry/content.halo.run/posts/{name}"), b"{}")
            .await
            .unwrap();
    }
    let first = pool
        .list_by_prefix_paged("/registry/content.halo.run/posts", None, 2)
        .await
        .unwrap();
    assert_eq!(first.len(), 2);
    let cursor = first.last().unwrap().name.clone();
    let next = pool
        .list_by_prefix_paged("/registry/content.halo.run/posts", Some(&cursor), 2)
        .await
        .unwrap();
    assert_eq!(next.len(), 2);
    let names: Vec<_> = next.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "/registry/content.halo.run/posts/c",
            "/registry/content.halo.run/posts/d",
        ]
    );
}

#[tokio::test]
async fn count_by_prefix() {
    let pool = pool().await;
    pool.create("/registry/content.halo.run/posts/a", b"{}")
        .await
        .unwrap();
    pool.create("/registry/content.halo.run/posts/b", b"{}")
        .await
        .unwrap();
    pool.create("/registry/users/admin", b"{}").await.unwrap();
    assert_eq!(
        pool.count_by_prefix("/registry/content.halo.run/posts")
            .await
            .unwrap(),
        2
    );
    assert_eq!(pool.count_by_prefix("/registry/users").await.unwrap(), 1);
    assert_eq!(pool.count_by_prefix("/registry/tags").await.unwrap(), 0);
}

#[tokio::test]
async fn fetch_many_returns_subset() {
    let pool = pool().await;
    pool.create("/registry/users/a", b"{}").await.unwrap();
    pool.create("/registry/users/b", b"{}").await.unwrap();
    pool.create("/registry/users/c", b"{}").await.unwrap();
    let rows = pool
        .fetch_many(&[
            "/registry/users/a".to_owned(),
            "/registry/users/c".to_owned(),
            "/registry/users/missing".to_owned(),
        ])
        .await
        .unwrap();
    let names: Vec<_> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["/registry/users/a", "/registry/users/c"]);
}

// ---------------------------------------------------------------------------
// Typed layer tests.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn typed_create_assigns_version_one() {
    let pool = pool().await;
    let store = TypedStore::new(&pool);
    let created = store.create(&ToyPost::new("hello", "Hello")).await.unwrap();
    assert_eq!(created.metadata.version, Some(1));
    assert_eq!(created.metadata.name, "hello");
    assert_eq!(created.spec.title, "Hello");
}

#[tokio::test]
async fn typed_fetch_mirrors_version() {
    let pool = pool().await;
    let store = TypedStore::new(&pool);
    store.create(&ToyPost::new("hi", "Hi")).await.unwrap();
    let fetched = store.fetch::<ToyPost>("hi").await.unwrap().unwrap();
    assert_eq!(fetched.metadata.version, Some(1));
    assert_eq!(fetched.spec.title, "Hi");
}

#[tokio::test]
async fn typed_update_round_trip() {
    let pool = pool().await;
    let store = TypedStore::new(&pool);
    let created = store.create(&ToyPost::new("p", "v1")).await.unwrap();

    let mut updated = created.clone();
    updated.spec.title = "v2".to_owned();
    let after = store.update(&updated).await.unwrap();
    assert_eq!(after.metadata.version, Some(2));
    assert_eq!(after.spec.title, "v2");

    // A stale write must conflict.
    let mut stale = created;
    stale.spec.title = "stale".to_owned();
    let err = store.update(&stale).await.unwrap_err();
    assert!(matches!(err, StoreError::OptimisticLock { .. }));
}

#[tokio::test]
async fn typed_list_returns_all() {
    let pool = pool().await;
    let store = TypedStore::new(&pool);
    for n in ["a", "b", "c"] {
        store.create(&ToyPost::new(n, n)).await.unwrap();
    }
    let listed = store.list::<ToyPost>().await.unwrap();
    let names: Vec<_> = listed.iter().map(|p| p.metadata.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
    for p in listed {
        assert_eq!(p.metadata.version, Some(1));
    }
}

#[tokio::test]
async fn typed_delete_returns_previous() {
    let pool = pool().await;
    let store = TypedStore::new(&pool);
    let created = store.create(&ToyPost::new("d", "del")).await.unwrap();
    let prev = store.delete(&created).await.unwrap();
    assert_eq!(prev.metadata.name, "d");
    assert!(store.fetch::<ToyPost>("d").await.unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Wire-format guarantee: the JSON we produce is shaped the way Halo expects.
// ---------------------------------------------------------------------------

#[test]
fn canonical_json_shape() {
    let mut p = ToyPost::new("hello", "Hello");
    p.metadata.labels = Some(BTreeMap::from([(
        "content.halo.run/published".to_owned(),
        "true".to_owned(),
    )]));
    let v: serde_json::Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["apiVersion"], "content.halo.run/v1alpha1");
    assert_eq!(v["kind"], "Post");
    assert_eq!(v["metadata"]["name"], "hello");
    assert_eq!(
        v["metadata"]["labels"]["content.halo.run/published"],
        "true"
    );
    assert_eq!(v["spec"]["title"], "Hello");
    // Null metadata fields must not appear (matches Halo's Jackson NON_NULL default).
    assert!(v["metadata"].get("generateName").is_none());
    assert!(v["metadata"].get("deletionTimestamp").is_none());
    assert!(v["metadata"].get("version").is_none());
}
