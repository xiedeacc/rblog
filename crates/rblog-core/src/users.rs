//! User accounts + login.
//!
//! v1 only handles classic email / password sign-in. Two-factor lives behind
//! a feature flag in the `UserSpec` schema; the UI just won't expose it.

use std::sync::Arc;

use chrono::Utc;
use rblog_auth::PasswordHasher;
use rblog_content::core::{User, UserSpec};
use rblog_index::IndexEngine;
use rblog_store::{AnyPool, TypedStore};
use serde::Serialize;

use crate::indexing::{remove, upsert};
use crate::{conflict, not_found, ServiceError};

#[derive(Clone)]
pub struct UserService {
    pool: AnyPool,
    index: Arc<IndexEngine>,
    hasher: Arc<PasswordHasher>,
}

impl UserService {
    pub fn new(pool: AnyPool, index: Arc<IndexEngine>, hasher: Arc<PasswordHasher>) -> Self {
        Self {
            pool,
            index,
            hasher,
        }
    }

    pub async fn create(&self, req: CreateUser) -> Result<User, ServiceError> {
        if req.name.trim().is_empty() {
            return Err(ServiceError::Validation("name must not be empty".into()));
        }
        if !req.email.contains('@') {
            return Err(ServiceError::Validation("email looks invalid".into()));
        }
        if req.password.len() < 8 {
            return Err(ServiceError::Validation(
                "password must be at least 8 chars".into(),
            ));
        }
        let store = TypedStore::new(&self.pool);
        if store.fetch::<User>(&req.name).await?.is_some() {
            return Err(conflict("User", req.name));
        }
        let hashed = self.hasher.hash(&req.password)?;
        let user = User::new(&req.name).with_spec(UserSpec {
            display_name: req.display_name,
            email: req.email,
            email_verified: true,
            password: Some(hashed),
            registered_at: Some(Utc::now()),
            ..UserSpec::default()
        });
        let saved = store.create(&user).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn get(&self, name: &str) -> Result<User, ServiceError> {
        let store = TypedStore::new(&self.pool);
        store
            .fetch::<User>(name)
            .await?
            .ok_or_else(|| not_found("User", name))
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let store = TypedStore::new(&self.pool);
        let u = store
            .fetch::<User>(name)
            .await?
            .ok_or_else(|| not_found("User", name))?;
        store.delete(&u).await?;
        remove::<User>(&self.index, name);
        Ok(())
    }

    pub async fn set_disabled(&self, name: &str, disabled: bool) -> Result<User, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let mut u = store
            .fetch::<User>(name)
            .await?
            .ok_or_else(|| not_found("User", name))?;
        if let Some(spec) = u.spec.as_mut() {
            spec.disabled = Some(disabled);
        }
        let saved = store.update(&u).await?;
        upsert(&self.index, &saved)?;
        Ok(saved)
    }

    pub async fn set_password(&self, name: &str, new_password: &str) -> Result<(), ServiceError> {
        if new_password.len() < 8 {
            return Err(ServiceError::Validation(
                "password must be at least 8 chars".into(),
            ));
        }
        let store = TypedStore::new(&self.pool);
        let mut u = store
            .fetch::<User>(name)
            .await?
            .ok_or_else(|| not_found("User", name))?;
        let hash = self.hasher.hash(new_password)?;
        if let Some(spec) = u.spec.as_mut() {
            spec.password = Some(hash);
        }
        let saved = store.update(&u).await?;
        upsert(&self.index, &saved)?;
        Ok(())
    }

    /// Verify `password` against the stored argon2id hash. Returns the user
    /// record on success and a typed [`ServiceError::Auth`] on failure.
    pub async fn authenticate(
        &self,
        name: &str,
        password: &str,
    ) -> Result<AuthenticatedUser, ServiceError> {
        let store = TypedStore::new(&self.pool);
        let user = store
            .fetch::<User>(name)
            .await?
            .ok_or_else(|| ServiceError::Auth("unknown user or wrong password".into()))?;
        if user.spec.as_ref().is_some_and(|s| s.disabled == Some(true)) {
            return Err(ServiceError::Auth("account disabled".into()));
        }
        let hash = user
            .spec
            .as_ref()
            .and_then(|s| s.password.clone())
            .ok_or_else(|| ServiceError::Auth("user has no password set".into()))?;
        let ok = self.hasher.verify(password, &hash)?;
        if !ok {
            return Err(ServiceError::Auth("unknown user or wrong password".into()));
        }
        Ok(AuthenticatedUser {
            name: user.metadata.name.clone(),
            display_name: user
                .spec
                .as_ref()
                .map(|s| s.display_name.clone())
                .unwrap_or_default(),
            email: user
                .spec
                .as_ref()
                .map(|s| s.email.clone())
                .unwrap_or_default(),
            user,
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CreateUser {
    pub name: String,
    pub display_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthenticatedUser {
    pub name: String,
    pub display_name: String,
    pub email: String,
    #[serde(skip)]
    pub user: User,
}
