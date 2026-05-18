//! Internal helpers for keeping the in-memory `IndexEngine` in sync with the
//! `TypedStore`.

use rblog_index::{IndexEngine, IndexedExt};
use rblog_scheme::Extension;

use crate::ServiceError;

/// Re-index `ext` after a successful write.
pub(crate) fn upsert<E: Extension + serde::Serialize>(
    index: &IndexEngine,
    ext: &E,
) -> Result<(), ServiceError> {
    let entry = IndexedExt::from_extension(ext)?;
    index.upsert_one(&E::gvk(), entry);
    Ok(())
}

/// Drop `name` from the index after a successful delete.
pub(crate) fn remove<E: Extension>(index: &IndexEngine, name: &str) {
    index.remove_one(&E::gvk(), name);
}

/// Resync every entry of a kind from the store. Useful at boot.
pub(crate) async fn resync_kind<E>(
    index: &IndexEngine,
    pool: &rblog_store::AnyPool,
) -> Result<usize, ServiceError>
where
    E: Extension + serde::Serialize + serde::de::DeserializeOwned,
{
    let store = rblog_store::TypedStore::new(pool);
    let items: Vec<E> = store.list::<E>().await?;
    let count = items.len();
    let entries: Vec<IndexedExt> = items
        .iter()
        .map(IndexedExt::from_extension)
        .collect::<Result<_, _>>()?;
    index.upsert_all(E::gvk(), entries);
    Ok(count)
}
