//! Typed layer: bridges raw rows to [`Extension`] structs.
//!
//! On read, the row's `version` column is mirrored into `metadata.version`,
//! matching Halo's `JSONExtensionConverter::convertFrom`.
//!
//! On write, the caller may pass a stale `metadata.version`; we use whatever
//! is in `metadata.version` as the expected concurrency token. After a
//! successful create / update, the freshly-bumped version is reflected back
//! into the returned struct's metadata.

use rblog_scheme::{build_store_name, Extension};
use serde::Serialize;

use crate::raw::{AnyPool, RawStore, StoreError};

/// High-level operations over the [`RawStore`] using typed Extensions.
pub struct TypedStore<'a> {
    pool: &'a AnyPool,
}

impl<'a> TypedStore<'a> {
    pub fn new(pool: &'a AnyPool) -> Self {
        Self { pool }
    }

    /// Read one Extension by name. Returns `None` if missing.
    pub async fn fetch<E: Extension>(&self, name: &str) -> Result<Option<E>, StoreError> {
        let store_name = build_store_name(&E::gvk(), name);
        let Some(row) = self.pool.fetch(&store_name).await? else {
            return Ok(None);
        };
        let mut decoded: E = decode(&row.data)?;
        decoded.metadata_mut().version = Some(row.version);
        Ok(Some(decoded))
    }

    /// List all Extensions of a kind.
    pub async fn list<E: Extension>(&self) -> Result<Vec<E>, StoreError> {
        let prefix = rblog_scheme::build_store_name_prefix(&E::gvk());
        let rows = self.pool.list_by_prefix(&prefix).await?;
        rows.into_iter()
            .map(|row| {
                let mut decoded: E = decode(&row.data)?;
                decoded.metadata_mut().version = Some(row.version);
                Ok(decoded)
            })
            .collect()
    }

    /// Insert. `metadata.version` is set to `1` on the returned struct.
    pub async fn create<E: Extension>(&self, ext: &E) -> Result<E, StoreError> {
        let store_name = ext.store_name();
        let bytes = serialize_canonical(ext)?;
        let row = self.pool.create(&store_name, &bytes).await?;
        let mut out: E = decode(&row.data)?;
        out.metadata_mut().version = Some(row.version);
        Ok(out)
    }

    /// Update with optimistic concurrency. The current `metadata.version` is
    /// used as the expected token. Returns the updated struct with a fresh
    /// version.
    pub async fn update<E: Extension>(&self, ext: &E) -> Result<E, StoreError> {
        let store_name = ext.store_name();
        let expected = ext
            .metadata()
            .version
            .ok_or_else(|| StoreError::OptimisticLock {
                name: store_name.clone(),
                expected: -1,
            })?;
        let bytes = serialize_canonical(ext)?;
        let row = self.pool.update(&store_name, expected, &bytes).await?;
        let mut out: E = decode(&row.data)?;
        out.metadata_mut().version = Some(row.version);
        Ok(out)
    }

    /// Delete with optimistic concurrency. Returns the row that was removed.
    pub async fn delete<E: Extension>(&self, ext: &E) -> Result<E, StoreError> {
        let store_name = ext.store_name();
        let expected = ext
            .metadata()
            .version
            .ok_or_else(|| StoreError::OptimisticLock {
                name: store_name.clone(),
                expected: -1,
            })?;
        let row = self.pool.delete(&store_name, expected).await?;
        let mut out: E = decode(&row.data)?;
        out.metadata_mut().version = Some(row.version);
        Ok(out)
    }
}

fn decode<E: Extension>(data: &[u8]) -> Result<E, StoreError> {
    serde_json::from_slice(data)
        .map_err(|e| StoreError::Sqlx(sqlx::Error::Decode(Box::new(JsonDecodeError(e)))))
}

fn serialize_canonical<E: Serialize>(e: &E) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(e)
        .map_err(|e| StoreError::Sqlx(sqlx::Error::Encode(Box::new(JsonDecodeError(e)))))
}

#[derive(Debug)]
struct JsonDecodeError(serde_json::Error);

impl std::fmt::Display for JsonDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for JsonDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}
