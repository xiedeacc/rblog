//! In-memory secondary index for Halo Extensions.
//!
//! ## Why
//!
//! Halo persists every Extension as a JSON blob in `extensions(name, data,
//! version)`. SQL-side filtering on a JSON blob is awkward and portable-by
//! "well, it works on MySQL 8.0" — neither attribute pleases the use cases
//! rblog needs (list published posts ordered by `spec.publishTime` desc,
//! show all comments approved on a given subjectRef, etc).
//!
//! Halo solves this with an in-process `IndexEngine` that maintains a sorted
//! map of `(label_key, label_value) -> Set<name>` (and similar for field
//! selectors and annotations) per kind. We replicate the engine without the
//! Spring-isms.
//!
//! ## Lifecycle
//!
//! At process start, the caller seeds the engine from the store
//! (`engine.upsert_all(gvk, entries)`). On every successful CRUD operation
//! through the store, the caller threads the change to the engine
//! (`upsert_one` / `remove_one`). The engine is a single-process cache; it is
//! the caller's responsibility to keep it in sync.
//!
//! ## Query model
//!
//! [`ListOptions`] composes:
//! - `label_selectors`: equality / inequality / set membership / existence,
//! - `field_selectors`: equality / inequality on a `serde_json` path,
//! - `sort`: stable sort by a typed path (string / number / instant),
//! - `page`: offset + limit pagination.
//!
//! Selectors AND together. An empty options struct returns every entry in
//! insertion order.

mod entry;
mod query;

pub use entry::IndexedExt;
pub use query::{
    FieldSelector, JsonPath, LabelSelector, ListOptions, ListResult, Page, Sort, SortDirection,
};

use std::collections::HashMap;

use parking_lot::RwLock;
use rblog_scheme::GroupVersionKind;
use thiserror::Error;

/// Crate-level errors.
#[derive(Debug, Error)]
pub enum IndexError {
    #[error("invalid index entry: {0}")]
    Invalid(String),
    #[error("invalid selector: {0}")]
    Selector(String),
}

/// Process-wide secondary index. Thread-safe via internal `RwLock`s.
///
/// The engine holds one [`KindIndex`] per registered GVK. Cross-kind queries
/// are not supported — each kind owns its own keyspace.
#[derive(Default)]
pub struct IndexEngine {
    by_kind: RwLock<HashMap<GroupVersionKind, RwLock<KindIndex>>>,
}

impl IndexEngine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of registered kinds (not entries).
    #[must_use]
    pub fn kind_count(&self) -> usize {
        self.by_kind.read().len()
    }

    /// Number of entries for a given kind.
    #[must_use]
    pub fn entry_count(&self, gvk: &GroupVersionKind) -> usize {
        self.by_kind
            .read()
            .get(gvk)
            .map_or(0, |k| k.read().entries.len())
    }

    /// Replace every entry for a kind. Used at boot or after a full resync.
    pub fn upsert_all(&self, gvk: GroupVersionKind, entries: impl IntoIterator<Item = IndexedExt>) {
        let mut new_idx = KindIndex::default();
        for e in entries {
            new_idx.entries.insert(e.name.clone(), e);
        }
        self.by_kind.write().insert(gvk, RwLock::new(new_idx));
    }

    /// Insert or replace a single entry.
    pub fn upsert_one(&self, gvk: &GroupVersionKind, entry: IndexedExt) {
        let map = self.by_kind.read();
        if let Some(kind_lock) = map.get(gvk) {
            kind_lock.write().entries.insert(entry.name.clone(), entry);
            return;
        }
        drop(map);
        // First-time write for this kind.
        let mut by_kind = self.by_kind.write();
        let kind_lock = by_kind
            .entry(*gvk)
            .or_insert_with(|| RwLock::new(KindIndex::default()));
        kind_lock.write().entries.insert(entry.name.clone(), entry);
    }

    /// Drop a single entry by name. Returns true iff it was present.
    pub fn remove_one(&self, gvk: &GroupVersionKind, name: &str) -> bool {
        if let Some(kind_lock) = self.by_kind.read().get(gvk) {
            return kind_lock.write().entries.remove(name).is_some();
        }
        false
    }

    /// Look up a single entry by name.
    #[must_use]
    pub fn get(&self, gvk: &GroupVersionKind, name: &str) -> Option<IndexedExt> {
        self.by_kind
            .read()
            .get(gvk)
            .and_then(|k| k.read().entries.get(name).cloned())
    }

    /// Run a list query. Filters apply with AND; sort and pagination are
    /// applied after filtering. Total count reflects the filtered set, not
    /// the page.
    pub fn list(
        &self,
        gvk: &GroupVersionKind,
        opts: &ListOptions,
    ) -> Result<ListResult, IndexError> {
        let map = self.by_kind.read();
        let Some(kind_lock) = map.get(gvk) else {
            return Ok(ListResult::default());
        };
        let kind = kind_lock.read();

        let mut matches: Vec<&IndexedExt> = kind
            .entries
            .values()
            .filter(|e| query::matches(e, opts))
            .collect();

        if let Some(sort) = &opts.sort {
            query::sort_in_place(&mut matches, sort);
        }

        let total = matches.len();
        let (offset, limit) = opts.page.map_or((0, usize::MAX), |p| (p.offset, p.limit));
        let items = matches
            .iter()
            .skip(offset)
            .take(limit)
            .map(|e| (*e).clone())
            .collect();
        Ok(ListResult { items, total })
    }
}

/// Per-kind storage. Currently a plain hash map keyed by name; richer
/// per-label / per-field side maps are an option if profiling demands it.
#[derive(Default)]
struct KindIndex {
    entries: HashMap<String, IndexedExt>,
}
