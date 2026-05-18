//! First-run bootstrap: create the super-admin role, the role binding, the
//! initial admin user, and the system config map with default site metadata.
//!
//! Idempotent: if any of the named extensions already exist, they're left
//! alone. Returns a [`BootstrapReport`] describing which records were
//! actually created.

use std::collections::BTreeMap;
use std::sync::Arc;

use rblog_auth::{
    bootstrap_admin_user, bootstrap_super_admin_role, bootstrap_super_admin_role_binding,
    PasswordHasher, SUPER_ADMIN_ROLE,
};
use rblog_content::core::{ConfigMap, Role, RoleBinding, User};
use rblog_index::IndexEngine;
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;

use crate::indexing::upsert;
use crate::settings::SYSTEM_CONFIGMAP;
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
    let store = TypedStore::new(pool);
    let mut report = BootstrapReport::default();

    if store.fetch::<Role>(SUPER_ADMIN_ROLE).await?.is_none() {
        let role = bootstrap_super_admin_role();
        let saved = store.create(&role).await?;
        upsert(index, &saved)?;
        report.super_admin_role_created = true;
    }

    let binding = bootstrap_super_admin_role_binding(&opts.admin_username);
    if store
        .fetch::<RoleBinding>(binding.metadata.name())
        .await?
        .is_none()
    {
        let saved = store.create(&binding).await?;
        upsert(index, &saved)?;
        report.super_admin_binding_created = true;
    }

    if store.fetch::<User>(&opts.admin_username).await?.is_none() {
        let mut user = bootstrap_admin_user(&opts.admin_username, &opts.admin_email);
        let hash = hasher.hash(&opts.admin_password)?;
        if let Some(spec) = user.spec.as_mut() {
            spec.password = Some(hash);
        }
        let saved = store.create(&user).await?;
        upsert(index, &saved)?;
        report.admin_user_created = true;
    }

    if store.fetch::<ConfigMap>(SYSTEM_CONFIGMAP).await?.is_none() {
        let mut data = BTreeMap::new();
        data.insert("site.title".to_owned(), opts.site_title.clone());
        if let Some(sub) = &opts.site_subtitle {
            data.insert("site.subtitle".to_owned(), sub.clone());
        }
        if let Some(url) = &opts.site_base_url {
            data.insert("site.baseUrl".to_owned(), url.clone());
        }
        let mut cm = ConfigMap::new(SYSTEM_CONFIGMAP);
        cm.data = Some(data);
        let saved = store.create(&cm).await?;
        upsert(index, &saved)?;
        report.system_configmap_created = true;
    }

    Ok(report)
}
