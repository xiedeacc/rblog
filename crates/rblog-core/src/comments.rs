//! Comment + Reply management with built-in moderation.
//!
//! Halo's comment model is "comment under post or page, reply under
//! comment". Both share the `BaseCommentSpec` (raw + rendered + owner +
//! approved/hidden flags + ip + ua).
//!
//! The moderation policy is straightforward in v1:
//!
//! - Anonymous comments start with `approved = false` and require admin
//!   approval before they show on the public site.
//! - Registered users (`owner.kind == "User"`) are auto-approved.
//! - Optional `auto_approve_anonymous = true` flag flips anon to instant.

use std::sync::Arc;

use ammonia::Builder;
use chrono::Utc;
use rblog_content::content::{
    BaseCommentSpec, Comment, CommentOwner, CommentSpec, Post, Reply, ReplySpec, SinglePage,
};
use rblog_content::infra::Ref;
use rblog_index::{FieldSelector, IndexEngine, LabelSelector, ListOptions, SortDirection};
use rblog_scheme::{Extension, GroupVersionKind};
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;
use uuid::Uuid;

use crate::indexing::{remove, upsert};
use crate::{not_found, ServiceError};

const APPROVED_LABEL: &str = "content.halo.run/approved";
const SUBJECT_KIND_LABEL: &str = "content.halo.run/subject-kind";
const SUBJECT_NAME_LABEL: &str = "content.halo.run/subject-name";

/// Heuristic anti-spam scoring for incoming comments.
///
/// Three signals contribute to the verdict:
///
/// 1. URL count — more than two links in a body of fewer than 200 chars is
///    overwhelmingly spam in practice.
/// 2. Keyword blocklist — case-insensitive substring match against a small
///    list of common spam terms.
/// 3. Body length — empty / one-word bodies are flagged for moderation.
///
/// The verdict ladder is deliberately simple: Block, RequireModeration, or
/// Allow. The HTTP layer is expected to map Block to a 422 and to keep
/// RequireModeration as the silent "queued" path.
#[derive(Debug, Clone)]
pub struct SpamHeuristic {
    pub blocklist: Vec<String>,
    pub max_links_short_body: usize,
}

impl Default for SpamHeuristic {
    fn default() -> Self {
        Self {
            blocklist: ["viagra", "casino", "porn", "loan", "crypto giveaway"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            max_links_short_body: 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpamVerdict {
    Allow,
    RequireModeration(String),
    Block(String),
}

impl SpamHeuristic {
    pub fn score(&self, raw: &str) -> SpamVerdict {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return SpamVerdict::Block("comment is empty".into());
        }
        let lower = trimmed.to_lowercase();
        if let Some(hit) = self.blocklist.iter().find(|w| lower.contains(w.as_str())) {
            return SpamVerdict::Block(format!("comment contains banned term `{hit}`"));
        }
        let link_count = lower.matches("http://").count() + lower.matches("https://").count();
        if link_count > self.max_links_short_body && trimmed.len() < 200 {
            return SpamVerdict::Block(format!("{link_count} links in a short body"));
        }
        if trimmed.split_whitespace().count() < 2 {
            return SpamVerdict::RequireModeration("very short body".into());
        }
        if link_count > 0 {
            return SpamVerdict::RequireModeration("contains link(s)".into());
        }
        SpamVerdict::Allow
    }
}

#[derive(Clone)]
pub struct CommentService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
    auto_approve_anonymous: bool,
    spam: SpamHeuristic,
}

impl CommentService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self {
            pool,
            index,
            auto_approve_anonymous: false,
            spam: SpamHeuristic::default(),
        }
    }

    #[must_use]
    pub fn with_auto_approve(mut self, on: bool) -> Self {
        self.auto_approve_anonymous = on;
        self
    }

    #[must_use]
    pub fn with_spam_heuristic(mut self, spam: SpamHeuristic) -> Self {
        self.spam = spam;
        self
    }

    /// Submit a top-level comment on `subject` (a `Post` or `SinglePage`).
    pub async fn submit(&self, new: NewComment) -> Result<Comment, ServiceError> {
        let raw = sanitize_comment(&new.raw)?;
        let spam_requires_moderation = match self.spam.score(&new.raw) {
            SpamVerdict::Block(reason) => return Err(ServiceError::Validation(reason)),
            SpamVerdict::RequireModeration(_) => true,
            SpamVerdict::Allow => false,
        };
        let (kind_gvk, kind_name) = kind_for(new.subject_kind.as_deref())?;
        let store = TypedStore::new(&self.pool);
        let now = Utc::now();
        let auto_approved = self.should_auto_approve(&new.owner);
        let approved = auto_approved && !spam_requires_moderation;

        let mut comment = Comment::new(Uuid::new_v4().to_string()).with_spec(CommentSpec {
            base: BaseCommentSpec {
                raw: new.raw.clone(),
                content: raw,
                owner: new.owner.clone(),
                user_agent: new.user_agent.clone(),
                ip_address: new.ip_address.clone(),
                approved,
                approved_time: approved.then_some(now),
                creation_time: Some(now),
                allow_notification: true,
                hidden: false,
                priority: 0,
                top: false,
            },
            subject_ref: Ref::of_gvk(new.subject_name.clone(), &kind_gvk),
            last_read_time: None,
        });
        comment
            .metadata
            .set_label(APPROVED_LABEL, if approved { "true" } else { "false" });
        comment.metadata.set_label(SUBJECT_KIND_LABEL, kind_name);
        comment
            .metadata
            .set_label(SUBJECT_NAME_LABEL, &new.subject_name);
        let saved = store.create(&comment).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    /// Reply to an existing comment.
    pub async fn reply(&self, comment_name: &str, new: NewComment) -> Result<Reply, ServiceError> {
        let raw = sanitize_comment(&new.raw)?;
        let spam_requires_moderation = match self.spam.score(&new.raw) {
            SpamVerdict::Block(reason) => return Err(ServiceError::Validation(reason)),
            SpamVerdict::RequireModeration(_) => true,
            SpamVerdict::Allow => false,
        };
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<Comment>(comment_name)
            .await?
            .ok_or_else(|| not_found("Comment", comment_name))?;
        let now = Utc::now();
        let auto_approved = self.should_auto_approve(&new.owner);
        let approved = auto_approved && !spam_requires_moderation;
        let mut reply = Reply::new(Uuid::new_v4().to_string()).with_spec(ReplySpec {
            base: BaseCommentSpec {
                raw: new.raw.clone(),
                content: raw,
                owner: new.owner.clone(),
                user_agent: new.user_agent.clone(),
                ip_address: new.ip_address.clone(),
                approved,
                approved_time: approved.then_some(now),
                creation_time: Some(now),
                allow_notification: true,
                hidden: false,
                priority: 0,
                top: false,
            },
            comment_name: comment_name.to_owned(),
            quote_reply: new.quote_reply.clone(),
        });
        reply
            .metadata
            .set_label(APPROVED_LABEL, if approved { "true" } else { "false" });
        let saved = store.create(&reply).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    /// Flip approval state of a single comment.
    pub async fn approve(&self, name: &str) -> Result<Comment, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut comment = store
            .fetch::<Comment>(name)
            .await?
            .ok_or_else(|| not_found("Comment", name))?;
        if let Some(spec) = comment.spec.as_mut() {
            spec.base.approved = true;
            spec.base.approved_time = Some(Utc::now());
        }
        comment.metadata.set_label(APPROVED_LABEL, "true");
        let saved = store.update(&comment).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn hide(&self, name: &str) -> Result<Comment, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut comment = store
            .fetch::<Comment>(name)
            .await?
            .ok_or_else(|| not_found("Comment", name))?;
        if let Some(spec) = comment.spec.as_mut() {
            spec.base.hidden = true;
        }
        let saved = store.update(&comment).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn show(&self, name: &str) -> Result<Comment, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut comment = store
            .fetch::<Comment>(name)
            .await?
            .ok_or_else(|| not_found("Comment", name))?;
        if let Some(spec) = comment.spec.as_mut() {
            spec.base.hidden = false;
        }
        let saved = store.update(&comment).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn approve_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut reply = store
            .fetch::<Reply>(name)
            .await?
            .ok_or_else(|| not_found("Reply", name))?;
        if let Some(spec) = reply.spec.as_mut() {
            spec.base.approved = true;
            spec.base.approved_time = Some(Utc::now());
        }
        reply.metadata.set_label(APPROVED_LABEL, "true");
        let saved = store.update(&reply).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn hide_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut reply = store
            .fetch::<Reply>(name)
            .await?
            .ok_or_else(|| not_found("Reply", name))?;
        if let Some(spec) = reply.spec.as_mut() {
            spec.base.hidden = true;
        }
        let saved = store.update(&reply).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn show_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut reply = store
            .fetch::<Reply>(name)
            .await?
            .ok_or_else(|| not_found("Reply", name))?;
        if let Some(spec) = reply.spec.as_mut() {
            spec.base.hidden = false;
        }
        let saved = store.update(&reply).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete_reply(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let reply = store
            .fetch::<Reply>(name)
            .await?
            .ok_or_else(|| not_found("Reply", name))?;
        store.delete(&reply).await?;
        remove::<Reply>(&self.index, name);
        Ok(())
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let comment = store
            .fetch::<Comment>(name)
            .await?
            .ok_or_else(|| not_found("Comment", name))?;
        store.delete(&comment).await?;
        remove::<Comment>(&self.index, name);
        Ok(())
    }

    /// List approved, non-hidden comments under a subject, newest first.
    pub fn public_thread(
        &self,
        subject_kind: &str,
        subject_name: &str,
    ) -> Result<Vec<Comment>, ServiceError> {
        let opts = ListOptions::default()
            .with_label(LabelSelector::Equals {
                key: APPROVED_LABEL.to_owned(),
                value: "true".to_owned(),
            })
            .with_label(LabelSelector::Equals {
                key: SUBJECT_KIND_LABEL.to_owned(),
                value: subject_kind.to_owned(),
            })
            .with_label(LabelSelector::Equals {
                key: SUBJECT_NAME_LABEL.to_owned(),
                value: subject_name.to_owned(),
            })
            .sorted_by("spec.creationTime", SortDirection::Desc);
        let res = self.index.list(&Comment::gvk(), &opts)?;
        let comments = res
            .items
            .into_iter()
            .map(|e| {
                serde_json::from_value(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode Comment: {err}")))
            })
            .collect::<Result<Vec<Comment>, ServiceError>>()?;
        Ok(comments
            .into_iter()
            .filter(|comment| {
                comment
                    .spec
                    .as_ref()
                    .is_some_and(|spec| !spec.base.hidden)
            })
            .collect())
    }

    pub fn admin_comments(&self) -> Result<Vec<Comment>, ServiceError> {
        let opts = ListOptions::default().sorted_by("spec.creationTime", SortDirection::Desc);
        let res = self.index.list(&Comment::gvk(), &opts)?;
        res.items
            .into_iter()
            .map(|e| {
                serde_json::from_value(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode Comment: {err}")))
            })
            .collect()
    }

    pub fn admin_replies(&self) -> Result<Vec<Reply>, ServiceError> {
        let opts = ListOptions::default().sorted_by("spec.creationTime", SortDirection::Desc);
        let res = self.index.list(&Reply::gvk(), &opts)?;
        res.items
            .into_iter()
            .map(|e| {
                serde_json::from_value(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode Reply: {err}")))
            })
            .collect()
    }

    /// Admin moderation queue: every unapproved top-level comment.
    pub fn moderation_queue(&self) -> Result<Vec<Comment>, ServiceError> {
        Ok(self
            .admin_comments()?
            .into_iter()
            .filter(|comment| {
                comment
                    .spec
                    .as_ref()
                    .is_some_and(|spec| !spec.base.approved && !spec.base.hidden)
            })
            .collect())
    }

    pub fn reply_moderation_queue(&self) -> Result<Vec<Reply>, ServiceError> {
        Ok(self
            .admin_replies()?
            .into_iter()
            .filter(|reply| {
                reply
                    .spec
                    .as_ref()
                    .is_some_and(|spec| !spec.base.approved && !spec.base.hidden)
            })
            .collect())
    }

    /// Approved, non-hidden replies for a comment, oldest first.
    pub fn replies(&self, comment_name: &str) -> Result<Vec<Reply>, ServiceError> {
        let opts = ListOptions::default()
            .with_label(LabelSelector::Equals {
                key: APPROVED_LABEL.to_owned(),
                value: "true".to_owned(),
            })
            .with_field(FieldSelector::Equals {
                path: "spec.commentName".to_owned(),
                value: serde_json::Value::String(comment_name.to_owned()),
            })
            .sorted_by("spec.creationTime", SortDirection::Asc);
        let res = self.index.list(&Reply::gvk(), &opts)?;
        let replies = res
            .items
            .into_iter()
            .map(|e| {
                serde_json::from_value(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode Reply: {err}")))
            })
            .collect::<Result<Vec<Reply>, ServiceError>>()?;
        Ok(replies
            .into_iter()
            .filter(|reply| {
                reply
                    .spec
                    .as_ref()
                    .is_some_and(|spec| !spec.base.hidden)
            })
            .collect())
    }

    fn should_auto_approve(&self, owner: &CommentOwner) -> bool {
        if owner.kind == "User" {
            return true;
        }
        self.auto_approve_anonymous
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewComment {
    pub subject_kind: Option<String>,
    pub subject_name: String,
    pub raw: String,
    pub owner: CommentOwner,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub quote_reply: Option<String>,
}

fn kind_for(kind: Option<&str>) -> Result<(GroupVersionKind, &'static str), ServiceError> {
    match kind.unwrap_or("Post") {
        "Post" => Ok((Post::gvk(), "Post")),
        "SinglePage" => Ok((SinglePage::gvk(), "SinglePage")),
        other => Err(ServiceError::Validation(format!(
            "unsupported comment subject kind `{other}`"
        ))),
    }
}

/// Conservative HTML sanitizer for user-submitted markdown.
fn sanitize_comment(raw: &str) -> Result<String, ServiceError> {
    if raw.trim().is_empty() {
        return Err(ServiceError::Validation("comment must not be empty".into()));
    }
    if raw.len() > 16_384 {
        return Err(ServiceError::Validation("comment exceeds 16 KiB".into()));
    }
    // Render markdown into HTML, then sanitize. We do this inline (no comrak
    // dependency in this crate) — the pipeline crate already exposes a
    // synchronous render, but for comments we prefer a minimal allow-list.
    let html = rblog_content::render::render_markdown(raw)
        .map_err(|e| ServiceError::Content(e.to_string()))?
        .html;
    Ok(Builder::new()
        .add_generic_attributes(["lang"])
        .clean(&html)
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spam_blocks_known_terms() {
        let h = SpamHeuristic::default();
        assert!(matches!(
            h.score("Buy cheap viagra now"),
            SpamVerdict::Block(_)
        ));
    }

    #[test]
    fn spam_blocks_link_floods() {
        let h = SpamHeuristic::default();
        assert!(matches!(
            h.score("hi http://a.com http://b.com http://c.com"),
            SpamVerdict::Block(_)
        ));
    }

    #[test]
    fn spam_flags_links_for_moderation() {
        let h = SpamHeuristic::default();
        let body = "Here is the project I mentioned: https://example.com/path. \
                    It works in any modern browser, no install required.";
        assert!(matches!(h.score(body), SpamVerdict::RequireModeration(_)));
    }

    #[test]
    fn spam_flags_one_word_replies() {
        let h = SpamHeuristic::default();
        assert!(matches!(h.score("nice"), SpamVerdict::RequireModeration(_)));
    }

    #[test]
    fn spam_allows_ordinary_comments() {
        let h = SpamHeuristic::default();
        assert_eq!(
            h.score("This is a thoughtful, link-free comment."),
            SpamVerdict::Allow
        );
    }
}

/// Public-facing comment shape. Strips IP and UA before serialization.
#[derive(Debug, Clone, Serialize)]
pub struct PublicComment {
    pub name: String,
    pub raw: String,
    pub content: String,
    pub owner_display_name: String,
    pub owner_kind: String,
    pub created_at: Option<chrono::DateTime<Utc>>,
    pub priority: i32,
    pub top: bool,
}

impl From<Comment> for PublicComment {
    fn from(c: Comment) -> Self {
        let spec = c.spec.unwrap_or_default();
        Self {
            name: c.metadata.name,
            raw: spec.base.raw,
            content: spec.base.content,
            owner_display_name: spec.base.owner.display_name.unwrap_or(spec.base.owner.name),
            owner_kind: spec.base.owner.kind,
            created_at: spec.base.creation_time,
            priority: spec.base.priority,
            top: spec.base.top,
        }
    }
}
