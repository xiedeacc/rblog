use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::Utc;
use rblog_content::core::{ConfigMap, Setting, SettingSpec};
use rblog_store::AnyPool;
use sqlx::Row;

use crate::{not_found, ServiceError};

pub const SYSTEM_CONFIGMAP: &str = "system";
pub const SITE_VISITS_KEY: &str = "site.visits";

#[derive(Clone)]
pub struct ConfigMapService {
    pool: AnyPool,
    index: Arc<rblog_index::IndexEngine>,
}

impl ConfigMapService {
    pub fn new(pool: AnyPool, index: Arc<rblog_index::IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn get(&self, name: &str) -> Result<ConfigMap, ServiceError> {
        if name == SYSTEM_CONFIGMAP {
            return self.system().await;
        }
        let prefix = format!("configmap.{name}.");
        let data = load_prefixed(&self.pool, &prefix).await?;
        if data.is_empty() {
            return Err(not_found("ConfigMap", name));
        }
        Ok(configmap(name, data))
    }

    pub async fn upsert(
        &self,
        name: &str,
        data: BTreeMap<String, String>,
    ) -> Result<ConfigMap, ServiceError> {
        if name == SYSTEM_CONFIGMAP {
            for (key, value) in &data {
                upsert_key(&self.pool, key, value).await?;
            }
            return self.indexed_configmap(name, data);
        }
        let prefix = format!("configmap.{name}.");
        delete_prefixed(&self.pool, &prefix).await?;
        for (key, value) in &data {
            upsert_key(&self.pool, &format!("{prefix}{key}"), value).await?;
        }
        self.indexed_configmap(name, data)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let prefix = if name == SYSTEM_CONFIGMAP {
            String::new()
        } else {
            format!("configmap.{name}.")
        };
        if prefix.is_empty() {
            return Err(ServiceError::Validation(
                "system config map cannot be deleted".to_owned(),
            ));
        }
        delete_prefixed(&self.pool, &prefix).await
    }

    pub async fn system(&self) -> Result<ConfigMap, ServiceError> {
        self.indexed_configmap(SYSTEM_CONFIGMAP, load_system(&self.pool).await?)
    }

    pub async fn system_value(&self, key: &str) -> Result<Option<String>, ServiceError> {
        let row = sqlx::query("SELECT value FROM site_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        Ok(row.and_then(|row| row.try_get("value").ok()))
    }

    pub async fn increment_site_visit(&self) -> Result<u64, ServiceError> {
        let current = self
            .system_value(SITE_VISITS_KEY)
            .await?
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or_default();
        let next = current.saturating_add(1);
        upsert_key(&self.pool, SITE_VISITS_KEY, &next.to_string()).await?;
        let _ = self.system().await?;
        Ok(next)
    }

    fn indexed_configmap(
        &self,
        name: &str,
        data: BTreeMap<String, String>,
    ) -> Result<ConfigMap, ServiceError> {
        let cm = configmap(name, data);
        crate::indexing::upsert(&self.index, &cm)?;
        Ok(cm)
    }
}

#[derive(Clone)]
pub struct SettingService {
    pool: AnyPool,
}

impl SettingService {
    pub fn new(pool: AnyPool, _index: Arc<rblog_index::IndexEngine>) -> Self {
        Self { pool }
    }

    pub async fn create(&self, name: &str, spec: SettingSpec) -> Result<Setting, ServiceError> {
        let setting = Setting::new(name).with_spec(spec);
        self.upsert(&setting).await
    }

    pub async fn get(&self, name: &str) -> Result<Setting, ServiceError> {
        let key = format!("setting.{name}");
        let raw = self
            .system_value(&key)
            .await?
            .ok_or_else(|| not_found("Setting", name))?;
        let spec = serde_json::from_str(&raw)
            .map_err(|e| ServiceError::Internal(format!("decode SettingSpec: {e}")))?;
        Ok(Setting::new(name).with_spec(spec))
    }

    pub async fn upsert(&self, setting: &Setting) -> Result<Setting, ServiceError> {
        let name = setting.metadata.name.clone();
        let raw = serde_json::to_string(&setting.spec.clone().unwrap_or_default())
            .map_err(|e| ServiceError::Internal(format!("encode SettingSpec: {e}")))?;
        upsert_key(&self.pool, &format!("setting.{name}"), &raw).await?;
        Ok(setting.clone())
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM site_settings WHERE key = ?")
            .bind(format!("setting.{name}"))
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("Setting", name));
        }
        Ok(())
    }

    async fn system_value(&self, key: &str) -> Result<Option<String>, ServiceError> {
        let row = sqlx::query("SELECT value FROM site_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        Ok(row.and_then(|row| row.try_get("value").ok()))
    }
}

fn configmap(name: &str, data: BTreeMap<String, String>) -> ConfigMap {
    let mut cm = ConfigMap::new(name);
    cm.data = Some(data);
    cm
}

async fn load_system(pool: &AnyPool) -> Result<BTreeMap<String, String>, ServiceError> {
    let rows = sqlx::query(
        "SELECT key, value FROM site_settings WHERE key NOT LIKE 'configmap.%' AND key NOT LIKE 'setting.%' ORDER BY key",
    )
    .fetch_all(sqlite(pool)?)
    .await
    .map_err(map_sqlx)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.get("key"),
                row.try_get("value").ok().flatten().unwrap_or_default(),
            )
        })
        .collect())
}

async fn load_prefixed(
    pool: &AnyPool,
    prefix: &str,
) -> Result<BTreeMap<String, String>, ServiceError> {
    let rows = sqlx::query("SELECT key, value FROM site_settings WHERE key LIKE ? ORDER BY key")
        .bind(format!("{prefix}%"))
        .fetch_all(sqlite(pool)?)
        .await
        .map_err(map_sqlx)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let key: String = row.get("key");
            (
                key.strip_prefix(prefix).unwrap_or(&key).to_owned(),
                row.try_get("value").ok().flatten().unwrap_or_default(),
            )
        })
        .collect())
}

async fn delete_prefixed(pool: &AnyPool, prefix: &str) -> Result<(), ServiceError> {
    sqlx::query("DELETE FROM site_settings WHERE key LIKE ?")
        .bind(format!("{prefix}%"))
        .execute(sqlite(pool)?)
        .await
        .map_err(map_sqlx)?;
    Ok(())
}

async fn upsert_key(pool: &AnyPool, key: &str, value: &str) -> Result<(), ServiceError> {
    sqlx::query(
        "INSERT INTO site_settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(Utc::now().to_rfc3339())
    .execute(sqlite(pool)?)
    .await
    .map_err(map_sqlx)?;
    Ok(())
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
