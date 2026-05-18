//! In-process registry of every known Extension kind.
//!
//! `SchemeRegistry` is the rblog analog of Halo's `SchemeManager`. Each kind
//! registers a [`Scheme`] containing its [`GroupVersionKind`] and the function
//! used to deserialize a row's `data` bytes back into a typed Extension.
//!
//! The registry is keyed both by [`GroupVersionKind`] (for typed lookups from
//! Rust) and by `(group, plural)` (for routing dynamic / unstructured requests
//! coming over HTTP).
//!
//! Registries are *append-only after startup* in the common case; the API also
//! supports late registration so plugins can announce new kinds at install time.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::RwLock;

use thiserror::Error;

use crate::{Extension, GroupVersionKind};

/// Errors returned by [`SchemeRegistry`].
#[derive(Debug, Error)]
pub enum SchemeError {
    #[error("scheme for {kind} (group={group:?}, plural={plural:?}) is already registered")]
    AlreadyRegistered {
        kind: &'static str,
        group: &'static str,
        plural: &'static str,
    },
    #[error("no scheme registered for GVK {0}")]
    UnknownGvk(String),
}

/// Runtime metadata about a single registered kind.
///
/// Holds the GVK plus a few function pointers that operate on `serde_json::Value`
/// representations of the kind. This lets the rest of the system (store, index,
/// HTTP handlers, plugin host) work generically against any registered kind
/// without knowing its concrete type.
pub struct Scheme {
    gvk: GroupVersionKind,
    type_id: TypeId,
    /// Type-erased deserialize: bytes → `Box<dyn Any>` of the concrete kind.
    decode: fn(&[u8]) -> Result<Box<dyn Any + Send + Sync>, serde_json::Error>,
    /// Type-erased re-encode: serde_json::Value → bytes, used when an unstructured
    /// payload (e.g. from HTTP) needs to be normalised by re-serializing through
    /// the typed kind.
    encode_value: fn(&serde_json::Value) -> Result<Vec<u8>, serde_json::Error>,
}

impl Scheme {
    /// Build a [`Scheme`] for a concrete [`Extension`] type.
    #[must_use]
    pub fn for_kind<E: Extension>() -> Self {
        Self {
            gvk: E::gvk(),
            type_id: TypeId::of::<E>(),
            decode: decode_to_any::<E>,
            encode_value: encode_value::<E>,
        }
    }

    #[must_use]
    pub fn gvk(&self) -> &GroupVersionKind {
        &self.gvk
    }

    #[must_use]
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Deserialize `data` bytes into the concrete kind type.
    ///
    /// Returns `None` if the caller asked for a different `E` than the one this
    /// `Scheme` was registered for.
    pub fn decode_as<E: Extension>(&self, data: &[u8]) -> Result<Option<E>, serde_json::Error> {
        if TypeId::of::<E>() != self.type_id {
            return Ok(None);
        }
        let any = (self.decode)(data)?;
        let typed = any
            .downcast::<E>()
            .expect("type id matched so downcast must succeed");
        Ok(Some(*typed))
    }

    /// Normalize a `serde_json::Value` payload through the typed representation.
    ///
    /// This is what the HTTP layer uses to validate inbound writes: the request
    /// JSON is parsed as the typed kind (which rejects unknown shapes and applies
    /// defaults) and re-serialized to canonical bytes for storage.
    pub fn normalize(&self, value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
        (self.encode_value)(value)
    }
}

fn decode_to_any<E: Extension>(
    data: &[u8],
) -> Result<Box<dyn Any + Send + Sync>, serde_json::Error> {
    let typed: E = serde_json::from_slice(data)?;
    Ok(Box::new(typed))
}

fn encode_value<E: Extension>(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    let typed: E = serde_json::from_value(value.clone())?;
    serde_json::to_vec(&typed)
}

/// The catalog of every known [`Scheme`]. Thread-safe; cheap to read.
#[derive(Default)]
pub struct SchemeRegistry {
    by_gvk: RwLock<HashMap<GroupVersionKind, Scheme>>,
    by_group_plural: RwLock<HashMap<(&'static str, &'static str), GroupVersionKind>>,
}

impl SchemeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `E` with this registry. Returns an error if a scheme with the
    /// same GVK is already present.
    pub fn register<E: Extension>(&self) -> Result<(), SchemeError> {
        let scheme = Scheme::for_kind::<E>();
        let gvk = scheme.gvk;
        let key = (gvk.group, gvk.plural);

        let mut by_gvk = self.by_gvk.write().expect("scheme registry poisoned");
        if by_gvk.contains_key(&gvk) {
            return Err(SchemeError::AlreadyRegistered {
                kind: gvk.kind,
                group: gvk.group,
                plural: gvk.plural,
            });
        }
        by_gvk.insert(gvk, scheme);
        self.by_group_plural
            .write()
            .expect("scheme registry poisoned")
            .insert(key, gvk);
        Ok(())
    }

    /// Look up a GVK by `(group, plural)`. The HTTP layer routes
    /// `/api/v1alpha1/<group>/<plural>` requests through this lookup.
    #[must_use]
    pub fn lookup_by_group_plural(&self, group: &str, plural: &str) -> Option<GroupVersionKind> {
        let g = self
            .by_group_plural
            .read()
            .expect("scheme registry poisoned");
        g.iter()
            .find(|((rg, rp), _)| *rg == group && *rp == plural)
            .map(|(_, gvk)| *gvk)
    }

    /// Run `f` against the registered scheme for the given GVK.
    ///
    /// We avoid handing out a `&Scheme` because the inner RwLock guard would
    /// outlive the borrow checker's patience; the closure pattern keeps
    /// everything tidy.
    pub fn with_scheme<R>(
        &self,
        gvk: &GroupVersionKind,
        f: impl FnOnce(&Scheme) -> R,
    ) -> Result<R, SchemeError> {
        let by_gvk = self.by_gvk.read().expect("scheme registry poisoned");
        let scheme = by_gvk
            .get(gvk)
            .ok_or_else(|| SchemeError::UnknownGvk(gvk.to_string()))?;
        Ok(f(scheme))
    }

    /// Number of registered schemes — handy in tests and for `/system/info`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_gvk.read().expect("scheme registry poisoned").len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Metadata;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct FakeSpec {
        title: String,
    }

    #[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    struct Fake {
        api_version: String,
        kind: String,
        metadata: Metadata,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spec: Option<FakeSpec>,
    }

    impl Extension for Fake {
        fn gvk() -> GroupVersionKind {
            GroupVersionKind::new("fake.halo.run", "v1alpha1", "Fake", "fakes", "fake")
        }
        fn metadata(&self) -> &Metadata {
            &self.metadata
        }
        fn metadata_mut(&mut self) -> &mut Metadata {
            &mut self.metadata
        }
    }

    #[test]
    fn register_and_lookup() {
        let reg = SchemeRegistry::new();
        reg.register::<Fake>().unwrap();
        assert_eq!(reg.len(), 1);

        let gvk = reg
            .lookup_by_group_plural("fake.halo.run", "fakes")
            .unwrap();
        assert_eq!(gvk, Fake::gvk());

        reg.with_scheme(&gvk, |s| {
            assert_eq!(s.gvk().kind, "Fake");
        })
        .unwrap();
    }

    #[test]
    fn register_rejects_duplicates() {
        let reg = SchemeRegistry::new();
        reg.register::<Fake>().unwrap();
        let err = reg.register::<Fake>().unwrap_err();
        assert!(matches!(
            err,
            SchemeError::AlreadyRegistered { kind: "Fake", .. }
        ));
    }

    #[test]
    fn decode_as_round_trip() {
        let reg = SchemeRegistry::new();
        reg.register::<Fake>().unwrap();

        let raw = br#"{
            "apiVersion": "fake.halo.run/v1alpha1",
            "kind": "Fake",
            "metadata": { "name": "n1" },
            "spec": { "title": "hello" }
        }"#;

        let decoded: Option<Fake> = reg
            .with_scheme(&Fake::gvk(), |s| s.decode_as::<Fake>(raw))
            .unwrap()
            .unwrap();
        let decoded = decoded.unwrap();
        assert_eq!(decoded.metadata.name, "n1");
        assert_eq!(decoded.spec.unwrap().title, "hello");
    }
}
