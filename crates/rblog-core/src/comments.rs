//! Comment + Reply management with built-in moderation.
//!
//! rblog stores comments under a post or page, and replies under comments.
//! Both share the same moderation fields: raw/rendered content, owner,
//! approval, hidden state, IP, and user agent.
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
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::indexing::{remove, upsert};
use crate::{not_found, ServiceError};

pub(crate) const APPROVED_LABEL: &str = "rblog.dev/approved";
pub(crate) const SUBJECT_KIND_LABEL: &str = "rblog.dev/subject-kind";
pub(crate) const SUBJECT_NAME_LABEL: &str = "rblog.dev/subject-name";

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
        let now = Utc::now();
        let auto_approved = self.should_auto_approve(&new.owner);
        let approved = auto_approved && !spam_requires_moderation;

        let mut comment = Comment::new(Uuid::new_v4().to_string()).with_spec(CommentSpec {
            base: BaseCommentSpec {
                raw: new.raw.clone(),
                content: raw.clone(),
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
        sqlx::query(
            r#"
            INSERT INTO comments (
                name, subject_kind, subject_name, parent_name, raw, html, owner_kind, owner_name,
                owner_display_name, user_agent, ip_address, approved, hidden, top, priority,
                quote_reply, created_at, approved_at
            )
            VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, NULL, ?, ?)
            "#,
        )
        .bind(comment.metadata.name.clone())
        .bind(kind_name)
        .bind(&new.subject_name)
        .bind(&new.raw)
        .bind(raw)
        .bind(&new.owner.kind)
        .bind(&new.owner.name)
        .bind(new.owner.display_name.as_deref())
        .bind(new.user_agent.as_deref())
        .bind(new.ip_address.as_deref())
        .bind(if approved { 1_i64 } else { 0_i64 })
        .bind(now.to_rfc3339())
        .bind(approved.then(|| now.to_rfc3339()))
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let saved = self.comment_by_name(comment.metadata.name()).await?;
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
        let parent = self.comment_by_name(comment_name).await?;
        let now = Utc::now();
        let auto_approved = self.should_auto_approve(&new.owner);
        let approved = auto_approved && !spam_requires_moderation;
        let mut reply = Reply::new(Uuid::new_v4().to_string()).with_spec(ReplySpec {
            base: BaseCommentSpec {
                raw: new.raw.clone(),
                content: raw.clone(),
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
            comment_name: parent.metadata.name.clone(),
            quote_reply: new.quote_reply.clone(),
        });
        reply
            .metadata
            .set_label(APPROVED_LABEL, if approved { "true" } else { "false" });
        sqlx::query(
            r#"
            INSERT INTO comments (
                name, subject_kind, subject_name, parent_name, raw, html, owner_kind, owner_name,
                owner_display_name, user_agent, ip_address, approved, hidden, top, priority,
                quote_reply, created_at, approved_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?, ?)
            "#,
        )
        .bind(reply.metadata.name.clone())
        .bind(parent_kind(&parent))
        .bind(parent_subject(&parent))
        .bind(&parent.metadata.name)
        .bind(&new.raw)
        .bind(raw)
        .bind(&new.owner.kind)
        .bind(&new.owner.name)
        .bind(new.owner.display_name.as_deref())
        .bind(new.user_agent.as_deref())
        .bind(new.ip_address.as_deref())
        .bind(if approved { 1_i64 } else { 0_i64 })
        .bind(new.quote_reply.as_deref())
        .bind(now.to_rfc3339())
        .bind(approved.then(|| now.to_rfc3339()))
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        let saved = self.reply_by_name(reply.metadata.name()).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    /// Flip approval state of a single comment.
    pub async fn approve(&self, name: &str) -> Result<Comment, ServiceError> {
        sqlx::query("UPDATE comments SET approved = 1, approved_at = ? WHERE name = ? AND parent_name IS NULL")
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.comment_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn hide(&self, name: &str) -> Result<Comment, ServiceError> {
        sqlx::query("UPDATE comments SET hidden = 1 WHERE name = ? AND parent_name IS NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.comment_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn show(&self, name: &str) -> Result<Comment, ServiceError> {
        sqlx::query("UPDATE comments SET hidden = 0 WHERE name = ? AND parent_name IS NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.comment_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn approve_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        sqlx::query("UPDATE comments SET approved = 1, approved_at = ? WHERE name = ? AND parent_name IS NOT NULL")
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.reply_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn hide_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        sqlx::query("UPDATE comments SET hidden = 1 WHERE name = ? AND parent_name IS NOT NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.reply_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn show_reply(&self, name: &str) -> Result<Reply, ServiceError> {
        sqlx::query("UPDATE comments SET hidden = 0 WHERE name = ? AND parent_name IS NOT NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        let saved = self.reply_by_name(name).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete_reply(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM comments WHERE name = ? AND parent_name IS NOT NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Reply", name));
        }
        remove::<Reply>(&self.index, name);
        Ok(())
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM comments WHERE name = ? AND parent_name IS NULL")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Comment", name));
        }
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
            .filter(|comment| comment.spec.as_ref().is_some_and(|spec| !spec.base.hidden))
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

    pub fn public_comment_count(&self) -> Result<usize, ServiceError> {
        Ok(self
            .admin_comments()?
            .into_iter()
            .filter(|comment| {
                comment
                    .spec
                    .as_ref()
                    .is_some_and(|spec| spec.base.approved && !spec.base.hidden)
            })
            .count())
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
            .filter(|reply| reply.spec.as_ref().is_some_and(|spec| !spec.base.hidden))
            .collect())
    }

    async fn comment_by_name(&self, name: &str) -> Result<Comment, ServiceError> {
        let row = sqlx::query("SELECT * FROM comments WHERE name = ? AND parent_name IS NULL")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?
            .ok_or_else(|| not_found("Comment", name))?;
        comment_from_row(row)
    }

    async fn reply_by_name(&self, name: &str) -> Result<Reply, ServiceError> {
        let row = sqlx::query("SELECT * FROM comments WHERE name = ? AND parent_name IS NOT NULL")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?
            .ok_or_else(|| not_found("Reply", name))?;
        reply_from_row(row)
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

fn comment_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Comment, ServiceError> {
    let name: String = row.get("name");
    let subject_kind: String = row.get("subject_kind");
    let subject_name: String = row.get("subject_name");
    let gvk = match subject_kind.as_str() {
        "SinglePage" => SinglePage::gvk(),
        _ => Post::gvk(),
    };
    let approved = row.get::<i64, _>("approved") != 0;
    let mut comment = Comment::new(name).with_spec(CommentSpec {
        base: base_from_row(&row),
        subject_ref: Ref::of_gvk(subject_name.clone(), &gvk),
        last_read_time: None,
    });
    comment
        .metadata
        .set_label(APPROVED_LABEL, if approved { "true" } else { "false" });
    comment
        .metadata
        .set_label(SUBJECT_KIND_LABEL, &subject_kind);
    comment
        .metadata
        .set_label(SUBJECT_NAME_LABEL, &subject_name);
    Ok(comment)
}

fn reply_from_row(row: sqlx::sqlite::SqliteRow) -> Result<Reply, ServiceError> {
    let mut reply = Reply::new(row.get::<String, _>("name")).with_spec(ReplySpec {
        base: base_from_row(&row),
        comment_name: row.get("parent_name"),
        quote_reply: row.try_get("quote_reply").ok().flatten(),
    });
    let approved = row.get::<i64, _>("approved") != 0;
    reply
        .metadata
        .set_label(APPROVED_LABEL, if approved { "true" } else { "false" });
    Ok(reply)
}

fn base_from_row(row: &sqlx::sqlite::SqliteRow) -> BaseCommentSpec {
    BaseCommentSpec {
        raw: row.get("raw"),
        content: row.get("html"),
        owner: CommentOwner {
            kind: row.try_get("owner_kind").ok().flatten().unwrap_or_default(),
            name: row.try_get("owner_name").ok().flatten().unwrap_or_default(),
            display_name: row.try_get("owner_display_name").ok().flatten(),
            annotations: None,
        },
        user_agent: row.try_get("user_agent").ok().flatten(),
        ip_address: row.try_get("ip_address").ok().flatten(),
        approved: row.get::<i64, _>("approved") != 0,
        approved_time: parse_dt(
            row.try_get::<Option<String>, _>("approved_at")
                .ok()
                .flatten(),
        ),
        creation_time: parse_dt(
            row.try_get::<Option<String>, _>("created_at")
                .ok()
                .flatten(),
        ),
        allow_notification: true,
        hidden: row.get::<i64, _>("hidden") != 0,
        priority: i32::try_from(row.get::<i64, _>("priority")).unwrap_or_default(),
        top: row.get::<i64, _>("top") != 0,
    }
}

fn parent_kind(comment: &Comment) -> String {
    comment
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(SUBJECT_KIND_LABEL))
        .cloned()
        .unwrap_or_else(|| "Post".to_owned())
}

fn parent_subject(comment: &Comment) -> String {
    comment
        .metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(SUBJECT_NAME_LABEL))
        .cloned()
        .unwrap_or_default()
}

fn parse_dt(raw: Option<String>) -> Option<chrono::DateTime<Utc>> {
    raw.and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

fn sqlite(pool: &AnyPool) -> Result<&sqlx::SqlitePool, ServiceError> {
    match pool {
        AnyPool::Sqlite(pool) => Ok(pool),
        AnyPool::Mysql(_) => Err(ServiceError::Internal(
            "refactor branch only supports sqlite".to_owned(),
        )),
    }
}

fn map_sqlx(error: sqlx::Error) -> ServiceError {
    ServiceError::Internal(error.to_string())
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
