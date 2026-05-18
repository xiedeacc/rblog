//! Kinds under `content.halo.run/v1alpha1`.
//!
//! These are the bread-and-butter blog kinds: posts, pages, taxonomy and
//! comments. Wire-compatible with Halo's Java POJOs in
//! `run.halo.app.core.extension.content.*`.

use chrono::{DateTime, Utc};
use rblog_scheme::GroupVersionKind;
use serde::{Deserialize, Serialize};

use crate::infra::{ConditionList, Ref};

const GROUP: &str = "content.halo.run";
const VERSION: &str = "v1alpha1";

// ---------------------------------------------------------------------------
// Shared enums
// ---------------------------------------------------------------------------

/// `Post.spec.visible` and `SinglePage.spec.visible`. Serializes as Halo's
/// Java enum names (`PUBLIC`, `INTERNAL`, `PRIVATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Visible {
    #[default]
    Public,
    Internal,
    Private,
}

/// `Post.status.phase` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PostPhase {
    Draft,
    #[serde(rename = "PENDING_APPROVAL")]
    PendingApproval,
    Published,
    Failed,
}

// ---------------------------------------------------------------------------
// Post
// ---------------------------------------------------------------------------

const POST_GVK: GroupVersionKind = GroupVersionKind::new(GROUP, VERSION, "Post", "posts", "post");

define_kind!(
    /// Blog post. Content lives in [`Snapshot`] extensions referenced by
    /// `spec.{base,head,release}_snapshot`.
    Post,
    gvk = POST_GVK,
    spec = PostSpec,
    status = PostStatus,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSpec {
    pub title: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub publish: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default = "default_true")]
    pub allow_comment: bool,
    #[serde(default)]
    pub visible: Visible,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub excerpt: Excerpt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_metas: Option<Vec<std::collections::BTreeMap<String, String>>>,
}

impl Default for PostSpec {
    fn default() -> Self {
        Self {
            title: String::new(),
            slug: String::new(),
            release_snapshot: None,
            head_snapshot: None,
            base_snapshot: None,
            owner: None,
            template: None,
            cover: None,
            deleted: false,
            publish: false,
            publish_time: None,
            pinned: false,
            allow_comment: true,
            visible: Visible::default(),
            priority: 0,
            excerpt: Excerpt::default(),
            categories: None,
            tags: None,
            html_metas: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<PostPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<ConditionList>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_progress: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_from_list: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modify_time: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    #[serde(default = "default_true")]
    pub auto_generate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

impl Default for Excerpt {
    fn default() -> Self {
        Self {
            auto_generate: true,
            raw: None,
        }
    }
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// SinglePage
// ---------------------------------------------------------------------------

const SINGLE_PAGE_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "SinglePage", "singlepages", "singlepage");

define_kind!(
    /// Standalone CMS page (not a blog post).
    SinglePage,
    gvk = SINGLE_PAGE_GVK,
    spec = SinglePageSpec,
    status = PostStatus,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SinglePageSpec {
    pub title: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub publish: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default = "default_true")]
    pub allow_comment: bool,
    #[serde(default)]
    pub visible: Visible,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub excerpt: Excerpt,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_metas: Option<Vec<std::collections::BTreeMap<String, String>>>,
}

impl Default for SinglePageSpec {
    fn default() -> Self {
        Self {
            title: String::new(),
            slug: String::new(),
            release_snapshot: None,
            head_snapshot: None,
            base_snapshot: None,
            owner: None,
            template: None,
            cover: None,
            deleted: false,
            publish: false,
            publish_time: None,
            pinned: false,
            allow_comment: true,
            visible: Visible::default(),
            priority: 0,
            excerpt: Excerpt::default(),
            html_metas: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tag
// ---------------------------------------------------------------------------

const TAG_GVK: GroupVersionKind = GroupVersionKind::new(GROUP, VERSION, "Tag", "tags", "tag");

define_kind!(
    /// Post tag.
    Tag,
    gvk = TAG_GVK,
    spec = TagSpec,
    status = TagStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSpec {
    pub display_name: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_post_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<i64>,
}

// ---------------------------------------------------------------------------
// Category
// ---------------------------------------------------------------------------

const CATEGORY_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Category", "categories", "category");

define_kind!(
    /// Post category.
    Category,
    gvk = CATEGORY_GVK,
    spec = CategorySpec,
    status = CategoryStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySpec {
    pub display_name: String,
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_template: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<String>>,
    #[serde(default)]
    pub prevent_parent_post_cascade_query: bool,
    #[serde(default)]
    pub hide_from_list: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_post_count: Option<i32>,
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

const SNAPSHOT_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Snapshot", "snapshots", "snapshot");

define_kind!(
    /// Versioned content snapshot. The actual post content lives here as
    /// `raw_patch` (markdown) + `content_patch` (rendered HTML), composed
    /// across a chain via `parent_snapshot_name`.
    ///
    /// Snapshot has no `status` field in Halo, so we keep it as the same
    /// `SnapshotStatus` empty stub for parity.
    Snapshot,
    gvk = SNAPSHOT_GVK,
    spec = SnapshotSpec,
    status = SnapshotStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSpec {
    pub subject_ref: Ref,
    /// One of `markdown` | `html` | `json` | `asciidoc` | `latex`. We target
    /// markdown in rblog.
    pub raw_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_patch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_patch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_snapshot_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modify_time: Option<DateTime<Utc>>,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contributors: Option<std::collections::BTreeSet<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotStatus {}

// ---------------------------------------------------------------------------
// Comment
// ---------------------------------------------------------------------------

const COMMENT_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Comment", "comments", "comment");

define_kind!(
    /// Top-level comment on a post/page.
    Comment,
    gvk = COMMENT_GVK,
    spec = CommentSpec,
    status = CommentStatus,
);

/// Common fields shared by `Comment.spec` and `Reply.spec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseCommentSpec {
    /// Raw user input.
    pub raw: String,
    /// Rendered HTML (sanitized).
    pub content: String,
    pub owner: CommentOwner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_time: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub top: bool,
    #[serde(default = "default_true")]
    pub allow_notification: bool,
    #[serde(default)]
    pub approved: bool,
    #[serde(default)]
    pub hidden: bool,
}

impl Default for BaseCommentSpec {
    fn default() -> Self {
        Self {
            raw: String::new(),
            content: String::new(),
            owner: CommentOwner::default(),
            user_agent: None,
            ip_address: None,
            approved_time: None,
            creation_time: None,
            priority: 0,
            top: false,
            allow_notification: true,
            approved: false,
            hidden: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentSpec {
    #[serde(flatten)]
    pub base: BaseCommentSpec,
    pub subject_ref: Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentOwner {
    /// `Email` for anonymous commenters; the Halo user GVK kind name (`User`)
    /// for logged-in commenters.
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reply_time: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_reply_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread_reply_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_new_reply: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<i64>,
}

// ---------------------------------------------------------------------------
// Reply
// ---------------------------------------------------------------------------

const REPLY_GVK: GroupVersionKind =
    GroupVersionKind::new(GROUP, VERSION, "Reply", "replies", "reply");

define_kind!(
    /// Threaded reply under a [`Comment`].
    Reply,
    gvk = REPLY_GVK,
    spec = ReplySpec,
    status = ReplyStatus,
);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplySpec {
    #[serde(flatten)]
    pub base: BaseCommentSpec,
    pub comment_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_reply: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rblog_scheme::Extension;

    #[test]
    fn post_gvk_matches_halo() {
        assert_eq!(Post::gvk().group, "content.halo.run");
        assert_eq!(Post::gvk().plural, "posts");
        assert_eq!(Post::gvk().kind, "Post");
    }

    #[test]
    fn post_wire_shape_matches_halo() {
        let post = Post::new("hello").with_spec(PostSpec {
            title: "Hello".to_owned(),
            slug: "hello".to_owned(),
            ..PostSpec::default()
        });
        let v = serde_json::to_value(&post).unwrap();
        assert_eq!(v["apiVersion"], "content.halo.run/v1alpha1");
        assert_eq!(v["kind"], "Post");
        assert_eq!(v["metadata"]["name"], "hello");
        assert_eq!(v["spec"]["title"], "Hello");
        assert_eq!(v["spec"]["slug"], "hello");
        assert_eq!(v["spec"]["visible"], "PUBLIC");
        assert_eq!(v["spec"]["allowComment"], true);
        assert_eq!(v["spec"]["excerpt"]["autoGenerate"], true);
        // No null status leaking through.
        assert!(v.get("status").is_none() || v["status"].is_null());
    }

    #[test]
    fn deserialize_halo_post_payload() {
        // Sample shaped like Halo's output for a published post.
        let raw = r#"{
            "apiVersion": "content.halo.run/v1alpha1",
            "kind": "Post",
            "metadata": {
                "name": "first",
                "version": 3,
                "labels": { "content.halo.run/published": "true" },
                "creationTimestamp": "2026-01-01T00:00:00Z"
            },
            "spec": {
                "title": "First",
                "slug": "first",
                "releaseSnapshot": "snap-1",
                "headSnapshot": "snap-1",
                "baseSnapshot": "snap-0",
                "owner": "admin",
                "deleted": false,
                "publish": true,
                "publishTime": "2026-01-01T00:01:00Z",
                "pinned": false,
                "allowComment": true,
                "visible": "PUBLIC",
                "priority": 0,
                "excerpt": { "autoGenerate": true },
                "categories": ["news"],
                "tags": ["intro", "welcome"]
            },
            "status": {
                "phase": "PUBLISHED",
                "permalink": "/archives/first",
                "commentsCount": 3
            }
        }"#;
        let p: Post = serde_json::from_str(raw).unwrap();
        assert_eq!(p.api_version, "content.halo.run/v1alpha1");
        let spec = p.spec.unwrap();
        assert_eq!(spec.title, "First");
        assert_eq!(spec.slug, "first");
        assert_eq!(spec.visible, Visible::Public);
        assert!(spec.publish);
        assert_eq!(spec.categories.as_deref(), Some(&["news".to_owned()][..]));
        let status = p.status.unwrap();
        assert_eq!(status.phase, Some(PostPhase::Published));
        assert_eq!(status.comments_count, Some(3));
        assert_eq!(p.metadata.version, Some(3));
        assert_eq!(p.metadata.label("content.halo.run/published"), Some("true"));
    }

    #[test]
    fn snapshot_round_trips() {
        let mut snap = Snapshot::new("snap-1").with_spec(SnapshotSpec {
            subject_ref: Ref::of_gvk("first", &Post::gvk()),
            raw_type: "markdown".to_owned(),
            raw_patch: Some("# Hello".to_owned()),
            content_patch: Some("<h1>Hello</h1>".to_owned()),
            parent_snapshot_name: None,
            last_modify_time: None,
            owner: "admin".to_owned(),
            contributors: None,
        });
        snap.metadata.version = Some(1);
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["spec"]["subjectRef"]["kind"], "Post");
        assert_eq!(v["spec"]["rawType"], "markdown");
        assert_eq!(v["spec"]["rawPatch"], "# Hello");
        assert_eq!(v["spec"]["owner"], "admin");
        let back: Snapshot = serde_json::from_value(v).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn comment_owner_serializes_clean() {
        let c = Comment::new("c1").with_spec(CommentSpec {
            base: BaseCommentSpec {
                raw: "Nice".to_owned(),
                content: "<p>Nice</p>".to_owned(),
                owner: CommentOwner {
                    kind: "Email".to_owned(),
                    name: "anon@example.com".to_owned(),
                    display_name: Some("Anon".to_owned()),
                    annotations: None,
                },
                approved: true,
                ..BaseCommentSpec::default()
            },
            subject_ref: Ref::of_gvk("first", &Post::gvk()),
            last_read_time: None,
        });
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["spec"]["owner"]["kind"], "Email");
        assert_eq!(v["spec"]["owner"]["name"], "anon@example.com");
        assert_eq!(v["spec"]["raw"], "Nice");
        assert_eq!(v["spec"]["approved"], true);
        assert_eq!(v["spec"]["subjectRef"]["kind"], "Post");
    }
}
