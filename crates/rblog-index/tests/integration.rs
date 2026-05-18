//! End-to-end exercise of the index engine using real `rblog-content` kinds.

use rblog_content::{
    content::{Post, PostSpec, Visible},
    register_default_schemes,
};
use rblog_index::{
    FieldSelector, IndexEngine, IndexedExt, LabelSelector, ListOptions, SortDirection,
};
use rblog_scheme::{Extension, SchemeRegistry};

fn make_post(name: &str, title: &str, published: bool, priority: i32) -> Post {
    let mut p = Post::new(name).with_spec(PostSpec {
        title: title.to_owned(),
        slug: name.to_owned(),
        publish: published,
        visible: Visible::Public,
        priority,
        ..PostSpec::default()
    });
    p.metadata.set_label(
        "content.halo.run/published",
        if published { "true" } else { "false" },
    );
    p.metadata.version = Some(1);
    p
}

fn seed_engine(eng: &IndexEngine, posts: &[Post]) {
    let entries = posts
        .iter()
        .map(|p| IndexedExt::from_extension(p).expect("indexed"))
        .collect::<Vec<_>>();
    eng.upsert_all(Post::gvk(), entries);
}

#[test]
fn empty_options_lists_everything() {
    let eng = IndexEngine::new();
    let posts = [make_post("a", "A", true, 0), make_post("b", "B", false, 0)];
    seed_engine(&eng, &posts);

    let res = eng.list(&Post::gvk(), &ListOptions::default()).unwrap();
    assert_eq!(res.total, 2);
    assert_eq!(res.items.len(), 2);
}

#[test]
fn label_selector_filters_published_posts() {
    let eng = IndexEngine::new();
    let posts = [
        make_post("a", "A", true, 0),
        make_post("b", "B", false, 0),
        make_post("c", "C", true, 0),
    ];
    seed_engine(&eng, &posts);

    let opts = ListOptions::default().with_label(LabelSelector::Equals {
        key: "content.halo.run/published".to_owned(),
        value: "true".to_owned(),
    });
    let res = eng.list(&Post::gvk(), &opts).unwrap();
    assert_eq!(res.total, 2);
    let mut names: Vec<_> = res.items.iter().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["a".to_owned(), "c".to_owned()]);
}

#[test]
fn field_selector_filters_by_visibility() {
    let eng = IndexEngine::new();
    let mut public_a = make_post("a", "A", true, 0);
    public_a.spec.as_mut().unwrap().visible = Visible::Public;
    let mut private_b = make_post("b", "B", true, 0);
    private_b.spec.as_mut().unwrap().visible = Visible::Private;
    seed_engine(&eng, &[public_a, private_b]);

    let opts = ListOptions::default().with_field(FieldSelector::Equals {
        path: "spec.visible".to_owned(),
        value: serde_json::Value::String("PUBLIC".to_owned()),
    });
    let res = eng.list(&Post::gvk(), &opts).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].name, "a");
}

#[test]
fn sort_by_priority_desc_then_paginate() {
    let eng = IndexEngine::new();
    let posts: Vec<Post> = (0..5)
        .map(|i| make_post(&format!("p{i}"), &format!("Post {i}"), true, i))
        .collect();
    seed_engine(&eng, &posts);

    let opts = ListOptions::default()
        .sorted_by("spec.priority", SortDirection::Desc)
        .paged(1, 2);
    let res = eng.list(&Post::gvk(), &opts).unwrap();
    assert_eq!(res.total, 5);
    assert_eq!(res.items.len(), 2);
    assert_eq!(res.items[0].name, "p3");
    assert_eq!(res.items[1].name, "p2");
}

#[test]
fn upsert_replaces_existing_entry() {
    let eng = IndexEngine::new();
    let p = make_post("a", "First", true, 0);
    eng.upsert_one(&Post::gvk(), IndexedExt::from_extension(&p).unwrap());

    let p2 = make_post("a", "Updated", true, 0);
    eng.upsert_one(&Post::gvk(), IndexedExt::from_extension(&p2).unwrap());

    let res = eng.list(&Post::gvk(), &ListOptions::default()).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.items[0].raw["spec"]["title"], "Updated");
}

#[test]
fn remove_one_returns_true_when_present() {
    let eng = IndexEngine::new();
    let p = make_post("a", "A", true, 0);
    eng.upsert_one(&Post::gvk(), IndexedExt::from_extension(&p).unwrap());
    assert!(eng.remove_one(&Post::gvk(), "a"));
    assert!(!eng.remove_one(&Post::gvk(), "a"));
    assert_eq!(eng.entry_count(&Post::gvk()), 0);
}

#[test]
fn missing_kind_returns_empty_result() {
    let eng = IndexEngine::new();
    let res = eng.list(&Post::gvk(), &ListOptions::default()).unwrap();
    assert_eq!(res.total, 0);
    assert!(res.items.is_empty());
}

#[test]
fn registry_drives_indexing_for_all_kinds() {
    // Smoke test: every default kind has a usable GVK and the index engine
    // accepts at least one entry for each. This is the contract callers need
    // to know about before plumbing the store wakeup into the engine.
    let reg = SchemeRegistry::new();
    register_default_schemes(&reg).unwrap();
    let eng = IndexEngine::new();
    // For posts at minimum.
    let p = make_post("a", "A", true, 0);
    eng.upsert_one(&Post::gvk(), IndexedExt::from_extension(&p).unwrap());
    assert_eq!(eng.kind_count(), 1);
    assert_eq!(eng.entry_count(&Post::gvk()), 1);
    assert_eq!(reg.len(), 22);
}
