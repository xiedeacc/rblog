//! Menu + MenuItem service.

use std::sync::Arc;

use rblog_content::core::{Menu, MenuItem};
use rblog_index::{IndexEngine, ListOptions};
use rblog_scheme::Extension;
use rblog_store::{AnyPool, TypedStore};

use crate::indexing::{remove, upsert};
use crate::{not_found, ServiceError};

#[derive(Clone)]
pub struct MenuService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
}

impl MenuService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>) -> Self {
        Self { pool, index }
    }

    pub async fn create_menu(&self, menu: &Menu) -> Result<Menu, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let saved = store.create(menu).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn update_menu(&self, menu: &Menu) -> Result<Menu, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let saved = store.update(menu).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete_menu(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let m = store
            .fetch::<Menu>(name)
            .await?
            .ok_or_else(|| not_found("Menu", name))?;
        store.delete(&m).await?;
        remove::<Menu>(&self.index, name);
        Ok(())
    }

    pub fn list_menus(&self) -> Result<Vec<Menu>, ServiceError> {
        let res = self.index.list(&Menu::gvk(), &ListOptions::default())?;
        res.items
            .into_iter()
            .map(|e| {
                serde_json::from_value::<Menu>(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode Menu: {err}")))
            })
            .collect()
    }

    pub async fn create_item(&self, item: &MenuItem) -> Result<MenuItem, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let saved = store.create(item).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn update_item(&self, item: &MenuItem) -> Result<MenuItem, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let saved = store.update(item).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn delete_item(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let m = store
            .fetch::<MenuItem>(name)
            .await?
            .ok_or_else(|| not_found("MenuItem", name))?;
        store.delete(&m).await?;
        remove::<MenuItem>(&self.index, name);
        Ok(())
    }

    pub fn list_items(&self) -> Result<Vec<MenuItem>, ServiceError> {
        let res = self.index.list(&MenuItem::gvk(), &ListOptions::default())?;
        res.items
            .into_iter()
            .map(|e| {
                serde_json::from_value::<MenuItem>(e.raw)
                    .map_err(|err| ServiceError::Internal(format!("decode MenuItem: {err}")))
            })
            .collect()
    }
}
