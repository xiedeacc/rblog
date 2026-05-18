//! `Setting` (form schema) + `ConfigMap` (form values) services, plus the
//! well-known `system` config map which holds blog-wide settings like
//! `site.title` and `site.baseUrl`.

use std::collections::BTreeMap;
use std::sync::Arc;

use rblog_content::core::{ConfigMap, Setting, SettingSpec};
use rblog_index::IndexEngine;
use rblog_store::{AnyPool, TypedStore};

use crate::indexing::{remove, upsert};
use crate::{conflict, not_found, ServiceError};

/// Stable name for the blog-wide config map.
pub const SYSTEM_CONFIGMAP: &str = "system";

#[derive(Clone)]
pub struct ConfigMapService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
}

impl ConfigMapService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn get(&self, name: &str) -> Result<ConfigMap, ServiceError> {
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<ConfigMap>(name)
            .await?
            .ok_or_else(|| not_found("ConfigMap", name))
    }

    pub async fn upsert(
        &self,
        name: &str,
        data: BTreeMap<String, String>,
    ) -> Result<ConfigMap, ServiceError> {
        let store = TypedStore::new(&self.pool);
        if let Some(mut existing) = store.fetch::<ConfigMap>(name).await? {
            existing.data = Some(data);
            let saved = store.update(&existing).await?;
            upsert(&self.index, &saved)?;
            Ok(saved)
        } else {
            let mut cm = ConfigMap::new(name);
            cm.data = Some(data);
            let saved = store.create(&cm).await?;
            upsert(&self.index, &saved)?;
            Ok(saved)
        }
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let cm = store
            .fetch::<ConfigMap>(name)
            .await?
            .ok_or_else(|| not_found("ConfigMap", name))?;
        store.delete(&cm).await?;
        remove::<ConfigMap>(&self.index, name);
        Ok(())
    }

    /// Look up the system-wide config map; returns an empty placeholder if
    /// none has been written yet.
    pub async fn system(&self) -> Result<ConfigMap, ServiceError> {
        let store = TypedStore::new(&self.pool);
        Ok(store
            .fetch::<ConfigMap>(SYSTEM_CONFIGMAP)
            .await?
            .unwrap_or_else(|| ConfigMap::new(SYSTEM_CONFIGMAP)))
    }

    /// Convenience accessor for a single key on the `system` ConfigMap.
    pub async fn system_value(&self, key: &str) -> Result<Option<String>, ServiceError> {
        let cm = self.system().await?;
        Ok(cm.data.and_then(|m| m.get(key).cloned()))
    }
}

#[derive(Clone)]
pub struct SettingService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
}

impl SettingService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create(&self, name: &str, spec: SettingSpec) -> Result<Setting, ServiceError> {
        let store = TypedStore::new(&self.pool);
        if store.fetch::<Setting>(name).await?.is_some() {
            return Err(conflict("Setting", name));
        }
        let setting = Setting::new(name).with_spec(spec);
        let saved = store.create(&setting).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn get(&self, name: &str) -> Result<Setting, ServiceError> {
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<Setting>(name)
            .await?
            .ok_or_else(|| not_found("Setting", name))
    }

    pub async fn upsert(&self, setting: &Setting) -> Result<Setting, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let saved = match setting.metadata.version {
            Some(_) => store.update(setting).await?,
            None => store.create(setting).await?,
        };
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let s = store
            .fetch::<Setting>(name)
            .await?
            .ok_or_else(|| not_found("Setting", name))?;
        store.delete(&s).await?;
        remove::<Setting>(&self.index, name);
        Ok(())
    }
}
