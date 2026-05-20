//! First-run bootstrap for the clean rblog schema.

use std::sync::Arc;

use chrono::Utc;
use rblog_auth::PasswordHasher;
use rblog_content::core::ConfigMap;
use rblog_index::IndexEngine;
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;

use crate::ServiceError;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BootstrapOptions {
    pub admin_username: String,
    pub admin_email: String,
    pub admin_password: String,
    #[serde(default = "default_site_title")]
    pub site_title: String,
    #[serde(default)]
    pub site_subtitle: Option<String>,
    #[serde(default)]
    pub site_base_url: Option<String>,
}

fn default_site_title() -> String {
    "rblog".to_owned()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BootstrapReport {
    pub super_admin_role_created: bool,
    pub super_admin_binding_created: bool,
    pub admin_user_created: bool,
    pub system_configmap_created: bool,
}

/// Run the bootstrap. Safe to call on every boot — the function checks for
/// existing records before writing.
pub async fn bootstrap_system(
    pool: &AnyPool,
    index: &Arc<IndexEngine>,
    hasher: &PasswordHasher,
    opts: &BootstrapOptions,
) -> Result<BootstrapReport, ServiceError> {
    if opts.admin_username.trim().is_empty() {
        return Err(ServiceError::Validation(
            "admin username must not be empty".into(),
        ));
    }
    if !opts.admin_email.contains('@') {
        return Err(ServiceError::Validation("admin email looks invalid".into()));
    }
    if opts.admin_password.len() < 8 {
        return Err(ServiceError::Validation(
            "admin password must be at least 8 chars".into(),
        ));
    }
    let mut report = BootstrapReport::default();

    let pool = sqlite(pool)?;
    let existing = sqlx::query("SELECT COUNT(*) AS count FROM users WHERE name = ?")
        .bind(&opts.admin_username)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx)?
        .get::<i64, _>("count");
    if existing == 0 {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO users (name, display_name, email, password_hash, disabled, registered_at, created_at, updated_at) VALUES (?, ?, ?, ?, 0, ?, ?, ?)",
        )
        .bind(&opts.admin_username)
        .bind(&opts.admin_username)
        .bind(&opts.admin_email)
        .bind(hasher.hash(&opts.admin_password)?)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(map_sqlx)?;
        report.admin_user_created = true;
    }
    // These booleans preserve the existing response contract. Roles are no
    // longer stored as separate objects; the first local user is the site admin.
    report.super_admin_role_created = report.admin_user_created;
    report.super_admin_binding_created = report.admin_user_created;

    let mut system_data = std::collections::BTreeMap::new();
    system_data.insert("site.title".to_owned(), opts.site_title.clone());
    report.system_configmap_created |= upsert_setting(pool, "site.title", &opts.site_title).await?;
    if let Some(subtitle) = &opts.site_subtitle {
        system_data.insert("site.subtitle".to_owned(), subtitle.clone());
        report.system_configmap_created |= upsert_setting(pool, "site.subtitle", subtitle).await?;
    }
    if let Some(base_url) = &opts.site_base_url {
        system_data.insert("site.baseUrl".to_owned(), base_url.clone());
        report.system_configmap_created |= upsert_setting(pool, "site.baseUrl", base_url).await?;
    }
    let mut cm = ConfigMap::new("system");
    cm.data = Some(system_data);
    crate::indexing::upsert(index, &cm)?;

    Ok(report)
}

async fn upsert_setting(
    pool: &sqlx::SqlitePool,
    key: &str,
    value: &str,
) -> Result<bool, ServiceError> {
    let exists = sqlx::query("SELECT COUNT(*) AS count FROM site_settings WHERE key = ?")
        .bind(key)
        .fetch_one(pool)
        .await
        .map_err(map_sqlx)?
        .get::<i64, _>("count")
        > 0;
    sqlx::query(
        "INSERT INTO site_settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await
    .map_err(map_sqlx)?;
    Ok(!exists)
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
