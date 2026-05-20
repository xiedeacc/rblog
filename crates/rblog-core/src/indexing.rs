//! Internal helpers for keeping the in-memory `IndexEngine` in sync with
//! relational-table projections.

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
