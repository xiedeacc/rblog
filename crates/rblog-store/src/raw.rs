//! Backend-specific SQL.
//!
//! Halo's `ExtensionStoreClient` operations boil down to:
//!
//! - `list_by_name_prefix(prefix)` — paged listing of one kind.
//! - `list_by_names(names)` — batch fetch (used after the index engine narrows).
//! - `fetch(name)` — single get.
//! - `create(name, data)` — insert; `version` starts at 1.
//! - `update(name, expected_version, data)` — bumps to `version+1`; conflict on row count 0.
//! - `delete(name, expected_version)` — same conflict rule.
//!
//! These are implemented identically for MySQL and SQLite; only the SQL
//! placeholder dialect differs. We avoid `sqlx::Any` because the macros and
//! `migrate!` story under `Any` lag the per-backend support.

use sqlx::{MySqlPool, SqlitePool};
use thiserror::Error;

/// Errors returned by the storage layer.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Optimistic concurrency violation: the row's `version` did not match the
    /// caller's expectation. The HTTP layer maps this to 409 Conflict.
    #[error("optimistic lock conflict on {name:?}: expected version {expected}, row was modified or missing")]
    OptimisticLock { name: String, expected: i64 },

    /// A row with the same `name` already exists.
    #[error("duplicate name {0:?}")]
    DuplicateName(String),

    /// The requested row is not present.
    #[error("no extension stored at {0:?}")]
    NotFound(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
}

/// One row from the `extensions` table — bytes, version, name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRow {
    pub name: String,
    pub data: Vec<u8>,
    pub version: i64,
}

/// Pool enum: one variant per supported backend.
///
/// Using an enum rather than `sqlx::AnyPool` keeps query-time dispatch
/// explicit and avoids the gaps in `Any`'s feature coverage (migrations,
/// LIMIT placeholders, BLOB handling).
#[derive(Debug, Clone)]
pub enum AnyPool {
    Mysql(MySqlPool),
    Sqlite(SqlitePool),
}

impl AnyPool {
    /// Borrow the backend label for tracing / metrics.
    #[must_use]
    pub fn backend(&self) -> &'static str {
        match self {
            Self::Mysql(_) => "mysql",
            Self::Sqlite(_) => "sqlite",
        }
    }

    /// Open a pool from a database URL. The scheme decides the backend:
    /// `mysql://...` → MySQL, `sqlite://...` or `sqlite::memory:` → SQLite.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        if let Some(rest) = url
            .strip_prefix("mysql://")
            .or_else(|| url.strip_prefix("mariadb://"))
        {
            let _ = rest;
            let pool = MySqlPool::connect(url).await?;
            Ok(Self::Mysql(pool))
        } else if url.starts_with("sqlite:") {
            // `sqlite::memory:` and `sqlite:///path` both work via `SqlitePool::connect`.
            let pool = SqlitePool::connect(url).await?;
            Ok(Self::Sqlite(pool))
        } else {
            Err(sqlx::Error::Configuration(
                format!("unsupported database URL scheme: {url}").into(),
            ))
        }
    }

    /// Close the pool.
    pub async fn close(&self) {
        match self {
            Self::Mysql(p) => p.close().await,
            Self::Sqlite(p) => p.close().await,
        }
    }

    /// Replace every row in `extensions` in a single transaction.
    ///
    /// This is intentionally raw: Halo backup restores already contain the
    /// wire-compatible JSON payloads, so typed validation would reject plugin
    /// or future upstream kinds that rblog does not know about yet.
    pub async fn replace_all(&self, rows: &[ExtensionRow]) -> Result<(), StoreError> {
        match self {
            Self::Mysql(p) => mysql_impl::replace_all(p, rows).await,
            Self::Sqlite(p) => sqlite_impl::replace_all(p, rows).await,
        }
    }
}

/// Raw byte-level CRUD against the `extensions` table.
///
/// This trait is the entire SQL surface for the rest of the system.
/// Concrete implementations are private — callers use [`AnyPool`].
#[allow(async_fn_in_trait)]
pub trait RawStore {
    async fn fetch(&self, name: &str) -> Result<Option<ExtensionRow>, StoreError>;
    async fn fetch_many(&self, names: &[String]) -> Result<Vec<ExtensionRow>, StoreError>;
    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<ExtensionRow>, StoreError>;
    async fn list_by_prefix_paged(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ExtensionRow>, StoreError>;
    async fn count_by_prefix(&self, prefix: &str) -> Result<i64, StoreError>;
    async fn create(&self, name: &str, data: &[u8]) -> Result<ExtensionRow, StoreError>;
    async fn update(
        &self,
        name: &str,
        expected_version: i64,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError>;
    async fn delete(&self, name: &str, expected_version: i64) -> Result<ExtensionRow, StoreError>;
}

impl RawStore for AnyPool {
    async fn fetch(&self, name: &str) -> Result<Option<ExtensionRow>, StoreError> {
        match self {
            Self::Mysql(p) => mysql_impl::fetch(p, name).await,
            Self::Sqlite(p) => sqlite_impl::fetch(p, name).await,
        }
    }

    async fn fetch_many(&self, names: &[String]) -> Result<Vec<ExtensionRow>, StoreError> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        match self {
            Self::Mysql(p) => mysql_impl::fetch_many(p, names).await,
            Self::Sqlite(p) => sqlite_impl::fetch_many(p, names).await,
        }
    }

    async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<ExtensionRow>, StoreError> {
        let pattern = like_prefix(prefix);
        match self {
            Self::Mysql(p) => mysql_impl::list_like(p, &pattern, None).await,
            Self::Sqlite(p) => sqlite_impl::list_like(p, &pattern, None).await,
        }
    }

    async fn list_by_prefix_paged(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let pattern = like_prefix(prefix);
        let bound = cursor.map(str::to_owned);
        match self {
            Self::Mysql(p) => {
                mysql_impl::list_like_paged(p, &pattern, bound.as_deref(), limit).await
            }
            Self::Sqlite(p) => {
                sqlite_impl::list_like_paged(p, &pattern, bound.as_deref(), limit).await
            }
        }
    }

    async fn count_by_prefix(&self, prefix: &str) -> Result<i64, StoreError> {
        let pattern = like_prefix(prefix);
        match self {
            Self::Mysql(p) => mysql_impl::count_like(p, &pattern).await,
            Self::Sqlite(p) => sqlite_impl::count_like(p, &pattern).await,
        }
    }

    async fn create(&self, name: &str, data: &[u8]) -> Result<ExtensionRow, StoreError> {
        match self {
            Self::Mysql(p) => mysql_impl::create(p, name, data).await,
            Self::Sqlite(p) => sqlite_impl::create(p, name, data).await,
        }
    }

    async fn update(
        &self,
        name: &str,
        expected_version: i64,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError> {
        match self {
            Self::Mysql(p) => mysql_impl::update(p, name, expected_version, data).await,
            Self::Sqlite(p) => sqlite_impl::update(p, name, expected_version, data).await,
        }
    }

    async fn delete(&self, name: &str, expected_version: i64) -> Result<ExtensionRow, StoreError> {
        match self {
            Self::Mysql(p) => mysql_impl::delete(p, name, expected_version).await,
            Self::Sqlite(p) => sqlite_impl::delete(p, name, expected_version).await,
        }
    }
}

/// Append the LIKE wildcard. The `name` column is collated `utf8mb4_bin` on
/// MySQL so we can rely on byte-prefix matching being exact, just like Halo.
fn like_prefix(prefix: &str) -> String {
    // Ensure exactly one trailing `/` before the wildcard so a prefix of
    // `/registry/users` does not match `/registry/usersettings/...`.
    let mut s = String::with_capacity(prefix.len() + 2);
    s.push_str(prefix.trim_end_matches('/'));
    s.push('/');
    // Escape LIKE wildcards in the input itself. `name` is a system-controlled
    // path so this is defense in depth.
    let escaped: String = s
        .chars()
        .flat_map(|c| match c {
            '%' | '_' | '\\' => vec!['\\', c].into_iter(),
            other => vec![other].into_iter(),
        })
        .collect();
    let mut out = escaped;
    out.push('%');
    out
}

// ---------------------------------------------------------------------------
// MySQL implementation
// ---------------------------------------------------------------------------

mod mysql_impl {
    use super::{ExtensionRow, StoreError};
    use sqlx::{MySqlPool, Row};

    pub(super) async fn fetch(
        pool: &MySqlPool,
        name: &str,
    ) -> Result<Option<ExtensionRow>, StoreError> {
        let row = sqlx::query("SELECT name, data, version FROM extensions WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(row_to_extension))
    }

    pub(super) async fn fetch_many(
        pool: &MySqlPool,
        names: &[String],
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let mut sql = String::from("SELECT name, data, version FROM extensions WHERE name IN (");
        for i in 0..names.len() {
            if i > 0 {
                sql.push(',');
            }
            sql.push('?');
        }
        sql.push_str(") ORDER BY name ASC");
        let mut q = sqlx::query(&sql);
        for n in names {
            q = q.bind(n);
        }
        let rows = q.fetch_all(pool).await?;
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn list_like(
        pool: &MySqlPool,
        like_pattern: &str,
        cursor: Option<&str>,
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let rows = match cursor {
            Some(c) => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\\\' AND name > ? \
                     ORDER BY name ASC",
                )
                .bind(like_pattern)
                .bind(c)
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\\\' \
                     ORDER BY name ASC",
                )
                .bind(like_pattern)
                .fetch_all(pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn list_like_paged(
        pool: &MySqlPool,
        like_pattern: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let rows = match cursor {
            Some(c) => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\\\' AND name > ? \
                     ORDER BY name ASC LIMIT ?",
                )
                .bind(like_pattern)
                .bind(c)
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\\\' \
                     ORDER BY name ASC LIMIT ?",
                )
                .bind(like_pattern)
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn count_like(
        pool: &MySqlPool,
        like_pattern: &str,
    ) -> Result<i64, StoreError> {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM extensions WHERE name LIKE ? ESCAPE '\\\\'")
                .bind(like_pattern)
                .fetch_one(pool)
                .await?;
        Ok(n)
    }

    pub(super) async fn create(
        pool: &MySqlPool,
        name: &str,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError> {
        let res = sqlx::query("INSERT INTO extensions (name, data, version) VALUES (?, ?, 1)")
            .bind(name)
            .bind(data)
            .execute(pool)
            .await;
        match res {
            Ok(_) => Ok(ExtensionRow {
                name: name.to_owned(),
                data: data.to_owned(),
                version: 1,
            }),
            Err(sqlx::Error::Database(e)) if is_mysql_dup(&*e) => {
                Err(StoreError::DuplicateName(name.to_owned()))
            }
            Err(e) => Err(StoreError::Sqlx(e)),
        }
    }

    pub(super) async fn update(
        pool: &MySqlPool,
        name: &str,
        expected_version: i64,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError> {
        let res = sqlx::query(
            "UPDATE extensions SET data = ?, version = version + 1 WHERE name = ? AND version = ?",
        )
        .bind(data)
        .bind(name)
        .bind(expected_version)
        .execute(pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        Ok(ExtensionRow {
            name: name.to_owned(),
            data: data.to_owned(),
            version: expected_version + 1,
        })
    }

    pub(super) async fn delete(
        pool: &MySqlPool,
        name: &str,
        expected_version: i64,
    ) -> Result<ExtensionRow, StoreError> {
        let row = sqlx::query("SELECT name, data, version FROM extensions WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| StoreError::NotFound(name.to_owned()))?;
        let prev = row_to_extension(row);
        if prev.version != expected_version {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        let res = sqlx::query("DELETE FROM extensions WHERE name = ? AND version = ?")
            .bind(name)
            .bind(expected_version)
            .execute(pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        Ok(prev)
    }

    pub(super) async fn replace_all(
        pool: &MySqlPool,
        rows: &[ExtensionRow],
    ) -> Result<(), StoreError> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM extensions")
            .execute(&mut *tx)
            .await?;
        for row in rows {
            sqlx::query("INSERT INTO extensions (name, data, version) VALUES (?, ?, ?)")
                .bind(&row.name)
                .bind(&row.data)
                .bind(row.version)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    fn row_to_extension(row: sqlx::mysql::MySqlRow) -> ExtensionRow {
        // Halo's MariaDB DDL leaves `version` nullable. Treat NULL as 0; rblog
        // itself always writes non-null versions starting at 1.
        ExtensionRow {
            name: row.get::<String, _>("name"),
            data: row.get::<Vec<u8>, _>("data"),
            version: row.get::<Option<i64>, _>("version").unwrap_or(0),
        }
    }

    fn is_mysql_dup(e: &dyn sqlx::error::DatabaseError) -> bool {
        // 1062 is MySQL's duplicate-key error code.
        e.code().as_deref() == Some("23000") || e.message().contains("Duplicate")
    }
}

// ---------------------------------------------------------------------------
// SQLite implementation
// ---------------------------------------------------------------------------

mod sqlite_impl {
    use super::{ExtensionRow, StoreError};
    use sqlx::{Row, SqlitePool};

    pub(super) async fn fetch(
        pool: &SqlitePool,
        name: &str,
    ) -> Result<Option<ExtensionRow>, StoreError> {
        let row = sqlx::query("SELECT name, data, version FROM extensions WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?;
        Ok(row.map(row_to_extension))
    }

    pub(super) async fn fetch_many(
        pool: &SqlitePool,
        names: &[String],
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let mut sql = String::from("SELECT name, data, version FROM extensions WHERE name IN (");
        for i in 0..names.len() {
            if i > 0 {
                sql.push(',');
            }
            sql.push('?');
        }
        sql.push_str(") ORDER BY name ASC");
        let mut q = sqlx::query(&sql);
        for n in names {
            q = q.bind(n);
        }
        let rows = q.fetch_all(pool).await?;
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn list_like(
        pool: &SqlitePool,
        like_pattern: &str,
        cursor: Option<&str>,
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let rows = match cursor {
            Some(c) => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\' AND name > ? \
                     ORDER BY name ASC",
                )
                .bind(like_pattern)
                .bind(c)
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\' \
                     ORDER BY name ASC",
                )
                .bind(like_pattern)
                .fetch_all(pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn list_like_paged(
        pool: &SqlitePool,
        like_pattern: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ExtensionRow>, StoreError> {
        let rows = match cursor {
            Some(c) => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\' AND name > ? \
                     ORDER BY name ASC LIMIT ?",
                )
                .bind(like_pattern)
                .bind(c)
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT name, data, version FROM extensions \
                     WHERE name LIKE ? ESCAPE '\\' \
                     ORDER BY name ASC LIMIT ?",
                )
                .bind(like_pattern)
                .bind(i64::from(limit))
                .fetch_all(pool)
                .await?
            }
        };
        Ok(rows.into_iter().map(row_to_extension).collect())
    }

    pub(super) async fn count_like(
        pool: &SqlitePool,
        like_pattern: &str,
    ) -> Result<i64, StoreError> {
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM extensions WHERE name LIKE ? ESCAPE '\\'")
                .bind(like_pattern)
                .fetch_one(pool)
                .await?;
        Ok(n)
    }

    pub(super) async fn create(
        pool: &SqlitePool,
        name: &str,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError> {
        let res = sqlx::query("INSERT INTO extensions (name, data, version) VALUES (?, ?, 1)")
            .bind(name)
            .bind(data)
            .execute(pool)
            .await;
        match res {
            Ok(_) => Ok(ExtensionRow {
                name: name.to_owned(),
                data: data.to_owned(),
                version: 1,
            }),
            Err(sqlx::Error::Database(e)) if is_sqlite_dup(&*e) => {
                Err(StoreError::DuplicateName(name.to_owned()))
            }
            Err(e) => Err(StoreError::Sqlx(e)),
        }
    }

    pub(super) async fn update(
        pool: &SqlitePool,
        name: &str,
        expected_version: i64,
        data: &[u8],
    ) -> Result<ExtensionRow, StoreError> {
        let res = sqlx::query(
            "UPDATE extensions SET data = ?, version = version + 1 WHERE name = ? AND version = ?",
        )
        .bind(data)
        .bind(name)
        .bind(expected_version)
        .execute(pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        Ok(ExtensionRow {
            name: name.to_owned(),
            data: data.to_owned(),
            version: expected_version + 1,
        })
    }

    pub(super) async fn delete(
        pool: &SqlitePool,
        name: &str,
        expected_version: i64,
    ) -> Result<ExtensionRow, StoreError> {
        let row = sqlx::query("SELECT name, data, version FROM extensions WHERE name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| StoreError::NotFound(name.to_owned()))?;
        let prev = row_to_extension(row);
        if prev.version != expected_version {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        let res = sqlx::query("DELETE FROM extensions WHERE name = ? AND version = ?")
            .bind(name)
            .bind(expected_version)
            .execute(pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(StoreError::OptimisticLock {
                name: name.to_owned(),
                expected: expected_version,
            });
        }
        Ok(prev)
    }

    pub(super) async fn replace_all(
        pool: &SqlitePool,
        rows: &[ExtensionRow],
    ) -> Result<(), StoreError> {
        let mut tx = pool.begin().await?;
        sqlx::query("DELETE FROM extensions")
            .execute(&mut *tx)
            .await?;
        for row in rows {
            sqlx::query("INSERT INTO extensions (name, data, version) VALUES (?, ?, ?)")
                .bind(&row.name)
                .bind(&row.data)
                .bind(row.version)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    fn row_to_extension(row: sqlx::sqlite::SqliteRow) -> ExtensionRow {
        // See the MySQL implementation: NULL `version` becomes 0.
        ExtensionRow {
            name: row.get::<String, _>("name"),
            data: row.get::<Vec<u8>, _>("data"),
            version: row.get::<Option<i64>, _>("version").unwrap_or(0),
        }
    }

    fn is_sqlite_dup(e: &dyn sqlx::error::DatabaseError) -> bool {
        // SQLite error: "UNIQUE constraint failed: extensions.name"
        e.message().contains("UNIQUE constraint failed")
    }
}

#[cfg(test)]
mod tests {
    use super::like_prefix;

    #[test]
    fn like_prefix_appends_slash_and_wildcard() {
        assert_eq!(like_prefix("/registry/users"), "/registry/users/%");
        assert_eq!(like_prefix("/registry/users/"), "/registry/users/%");
    }

    #[test]
    fn like_prefix_escapes_wildcards() {
        // Defensive: even though our store names never contain wildcards.
        let p = like_prefix("/registry/foo%/bar_baz");
        assert!(p.contains("foo\\%"));
        assert!(p.contains("bar\\_baz"));
    }
}
