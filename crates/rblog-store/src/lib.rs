//! Storage primitives for rblog's relational tables and legacy imports.
//!
//! This crate offers two layers:
//!
//! - [`raw`] — backend-specific SQL against MySQL and SQLite, exposed through
//!   the [`AnyPool`] enum and the [`RawStore`] trait. Operates on raw bytes;
//!   no knowledge of Extension types.
//! - [`typed`] — a thin layer over [`raw`] that uses [`rblog_scheme::Extension`]
//!   to (de)serialize the JSON payload to and from typed Rust structs, including
//!   mirroring the `version` column into `metadata.version`.
//!
//! The wire format and storage rules are documented in `TECH_REPORT.md` §6.

pub mod raw;
pub mod typed;

pub use raw::{AnyPool, ExtensionRow, RawStore, StoreError};
pub use typed::TypedStore;

/// Re-exported migration directories so callers can pick a dialect at boot.
pub mod migrations {
    /// MySQL/MariaDB migration set. Mirrors Halo's `schema-mariadb.sql`.
    pub static MYSQL: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/mysql");
    /// SQLite migration set. rblog-only; Halo doesn't ship SQLite.
    pub static SQLITE: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/sqlite");
}

/// Run the appropriate migrations for the underlying [`AnyPool`].
///
/// Useful at process startup: one call covers both backends.
pub async fn run_migrations(pool: &AnyPool) -> Result<(), sqlx::migrate::MigrateError> {
    match pool {
        AnyPool::Mysql(p) => migrations::MYSQL.run(p).await,
        AnyPool::Sqlite(p) => migrations::SQLITE.run(p).await,
    }
}
