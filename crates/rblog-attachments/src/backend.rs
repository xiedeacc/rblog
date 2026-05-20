//! Storage backend abstraction.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutPayload};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("object_store: {0}")]
    Inner(#[from] object_store::Error),
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid path `{0}`")]
    InvalidPath(String),
}

/// Result of a successful put.
#[derive(Debug, Clone)]
pub struct PutResult {
    /// Storage-internal key (e.g. `2026/05/abc-photo.jpg`).
    pub key: String,
    /// Public-facing URL the browser fetches.
    pub url: String,
    /// Server-computed byte length.
    pub size: u64,
}

/// Object listing entry.
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub key: String,
    pub url: String,
    pub size: u64,
}

/// Backend-agnostic storage trait.
#[async_trait]
pub trait StorageBackend: Send + Sync + std::fmt::Debug {
    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, StorageError>;
    async fn get(&self, key: &str) -> Result<Bytes, StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn list(&self, prefix: Option<&str>) -> Result<Vec<ObjectMetadata>, StorageError>;
    fn public_url(&self, key: &str) -> String;
    fn label(&self) -> &str;
}

/// Concrete storage construction. Wraps either a local FS or an S3 client
/// behind the same trait via `Arc<dyn StorageBackend>`.
#[derive(Debug, Clone)]
pub enum Storage {
    Local {
        root: PathBuf,
        public_prefix: String,
    },
    S3 {
        bucket: String,
        endpoint: Option<String>,
        region: Option<String>,
        access_key: Option<String>,
        secret_key: Option<String>,
        public_base_url: String,
    },
}

impl Storage {
    pub fn build(&self) -> Result<Arc<dyn StorageBackend>, StorageError> {
        match self {
            Self::Local {
                root,
                public_prefix,
            } => {
                std::fs::create_dir_all(root)?;
                let inner = LocalFileSystem::new_with_prefix(root)?;
                let prefix = public_prefix.trim_end_matches('/').to_owned();
                Ok(Arc::new(LocalStorage {
                    store: Arc::new(inner),
                    public_prefix: prefix,
                    root: root.clone(),
                }))
            }
            Self::S3 {
                bucket,
                endpoint,
                region,
                access_key,
                secret_key,
                public_base_url,
            } => {
                let mut builder = AmazonS3Builder::new().with_bucket_name(bucket);
                if let Some(endpoint) = endpoint {
                    builder = builder.with_endpoint(endpoint).with_allow_http(true);
                }
                if let Some(region) = region {
                    builder = builder.with_region(region);
                }
                if let (Some(ak), Some(sk)) = (access_key.as_deref(), secret_key.as_deref()) {
                    builder = builder.with_access_key_id(ak).with_secret_access_key(sk);
                }
                let inner = builder
                    .build()
                    .map_err(|e| StorageError::Config(e.to_string()))?;
                // Validate the public URL so we fail early.
                Url::parse(public_base_url)
                    .map_err(|e| StorageError::Config(format!("invalid public_base_url: {e}")))?;
                Ok(Arc::new(S3Storage {
                    store: Arc::new(inner),
                    bucket: bucket.clone(),
                    public_base_url: public_base_url.trim_end_matches('/').to_owned(),
                }))
            }
        }
    }
}

#[derive(Debug)]
struct LocalStorage {
    store: Arc<LocalFileSystem>,
    public_prefix: String,
    root: PathBuf,
}

#[async_trait]
impl StorageBackend for LocalStorage {
    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, StorageError> {
        let path = object_path(key)?;
        let size = bytes.len() as u64;
        self.store.put(&path, PutPayload::from(bytes)).await?;
        Ok(PutResult {
            key: key.to_owned(),
            url: self.public_url(key),
            size,
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = object_path(key)?;
        let bytes = self.store.get(&path).await?.bytes().await?;
        Ok(bytes)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = object_path(key)?;
        self.store.delete(&path).await?;
        Ok(())
    }

    async fn list(&self, prefix: Option<&str>) -> Result<Vec<ObjectMetadata>, StorageError> {
        use futures::stream::StreamExt;
        let prefix_path = prefix.map(object_path).transpose()?;
        let mut stream = self.store.list(prefix_path.as_ref());
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            let meta = item?;
            out.push(ObjectMetadata {
                key: meta.location.to_string(),
                url: self.public_url(meta.location.as_ref()),
                size: meta.size as u64,
            });
        }
        Ok(out)
    }

    fn public_url(&self, key: &str) -> String {
        format!("{}/{}", self.public_prefix, key)
    }

    fn label(&self) -> &str {
        const LABEL: &str = "local";
        LABEL
    }
}

impl LocalStorage {
    #[allow(dead_code)]
    fn fs_path(&self, key: &str) -> PathBuf {
        let mut p = self.root.clone();
        for tok in key.split('/') {
            if tok.is_empty() {
                continue;
            }
            p.push(tok);
        }
        p
    }
}

#[derive(Debug)]
struct S3Storage {
    store: Arc<dyn ObjectStore>,
    bucket: String,
    public_base_url: String,
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn put(&self, key: &str, bytes: Bytes) -> Result<PutResult, StorageError> {
        let path = object_path(key)?;
        let size = bytes.len() as u64;
        self.store.put(&path, PutPayload::from(bytes)).await?;
        Ok(PutResult {
            key: key.to_owned(),
            url: self.public_url(key),
            size,
        })
    }

    async fn get(&self, key: &str) -> Result<Bytes, StorageError> {
        let path = object_path(key)?;
        let bytes = self.store.get(&path).await?.bytes().await?;
        Ok(bytes)
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = object_path(key)?;
        self.store.delete(&path).await?;
        Ok(())
    }

    async fn list(&self, prefix: Option<&str>) -> Result<Vec<ObjectMetadata>, StorageError> {
        use futures::stream::StreamExt;
        let prefix_path = prefix.map(object_path).transpose()?;
        let mut stream = self.store.list(prefix_path.as_ref());
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            let meta = item?;
            out.push(ObjectMetadata {
                key: meta.location.to_string(),
                url: self.public_url(meta.location.as_ref()),
                size: meta.size as u64,
            });
        }
        Ok(out)
    }

    fn public_url(&self, key: &str) -> String {
        format!("{}/{}", self.public_base_url, key)
    }

    fn label(&self) -> &str {
        // Keep the label dynamic so log lines distinguish buckets.
        Box::leak(format!("s3:{}", self.bucket).into_boxed_str())
    }
}

fn object_path(key: &str) -> Result<ObjectPath, StorageError> {
    if key.starts_with('/')
        || key.contains('\\')
        || key.split('/').any(|segment| segment.is_empty() || segment == "..")
    {
        return Err(StorageError::InvalidPath(key.to_owned()));
    }
    Ok(ObjectPath::from(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let backend = Storage::Local {
            root: tmp.path().to_owned(),
            public_prefix: "/uploads".into(),
        }
        .build()
        .unwrap();
        let key = "x/y/hello.txt";
        let res = backend.put(key, Bytes::from_static(b"hi")).await.unwrap();
        assert_eq!(res.size, 2);
        assert_eq!(res.url, "/uploads/x/y/hello.txt");
        let got = backend.get(key).await.unwrap();
        assert_eq!(&got[..], b"hi");
        let listed = backend.list(Some("x")).await.unwrap();
        assert_eq!(listed.len(), 1);
        backend.delete(key).await.unwrap();
        let listed = backend.list(Some("x")).await.unwrap();
        assert_eq!(listed.len(), 0);
    }
}
