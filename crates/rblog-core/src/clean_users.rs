use std::sync::Arc;

use chrono::{DateTime, Utc};
use rblog_auth::PasswordHasher;
use rblog_content::core::{User, UserSpec};
use rblog_store::AnyPool;
use serde::Serialize;
use sqlx::Row;

use crate::{conflict, not_found, ServiceError};

#[derive(Clone)]
pub struct UserService {
    pool: AnyPool,
    hasher: Arc<PasswordHasher>,
}

impl UserService {
    pub fn new(
        pool: AnyPool,
        _index: Arc<rblog_index::IndexEngine>,
        hasher: Arc<PasswordHasher>,
    ) -> Self {
        Self { pool, hasher }
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
        if self.find_row(&req.name).await?.is_some() {
            return Err(conflict("User", req.name));
        }
        let now = Utc::now().to_rfc3339();
        let hash = self.hasher.hash(&req.password)?;
        sqlx::query(
            "INSERT INTO users (name, display_name, email, password_hash, disabled, registered_at, created_at, updated_at) VALUES (?, ?, ?, ?, 0, ?, ?, ?)",
        )
        .bind(&req.name)
        .bind(&req.display_name)
        .bind(&req.email)
        .bind(hash)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(sqlite(&self.pool)?)
        .await
        .map_err(map_sqlx)?;
        self.get(&req.name).await
    }

    pub async fn get(&self, name: &str) -> Result<User, ServiceError> {
        let row = self
            .find_row(name)
            .await?
            .ok_or_else(|| not_found("User", name))?;
        user_from_row(row)
    }

    pub async fn list(&self) -> Result<Vec<User>, ServiceError> {
        sqlx::query("SELECT * FROM users ORDER BY registered_at ASC, name ASC")
            .fetch_all(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?
            .into_iter()
            .map(user_from_row)
            .collect()
    }

    pub async fn delete(&self, name: &str) -> Result<(), ServiceError> {
        let res = sqlx::query("DELETE FROM users WHERE name = ?")
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("User", name));
        }
        Ok(())
    }

    pub async fn set_disabled(&self, name: &str, disabled: bool) -> Result<User, ServiceError> {
        let res = sqlx::query("UPDATE users SET disabled = ?, updated_at = ? WHERE name = ?")
            .bind(if disabled { 1_i64 } else { 0_i64 })
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("User", name));
        }
        self.get(name).await
    }

    pub async fn set_password(&self, name: &str, new_password: &str) -> Result<(), ServiceError> {
        if new_password.len() < 8 {
            return Err(ServiceError::Validation(
                "password must be at least 8 chars".into(),
            ));
        }
        let hash = self.hasher.hash(new_password)?;
        let res = sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE name = ?")
            .bind(hash)
            .bind(Utc::now().to_rfc3339())
            .bind(name)
            .execute(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)?;
        if res.rows_affected() == 0 {
            return Err(not_found("User", name));
        }
        Ok(())
    }

    pub async fn authenticate(
        &self,
        name: &str,
        password: &str,
    ) -> Result<AuthenticatedUser, ServiceError> {
        let row = self
            .find_row(name)
            .await?
            .ok_or_else(|| ServiceError::Auth("unknown user or wrong password".into()))?;
        if row.get::<i64, _>("disabled") != 0 {
            return Err(ServiceError::Auth("account disabled".into()));
        }
        let hash = row
            .try_get::<Option<String>, _>("password_hash")
            .ok()
            .flatten()
            .ok_or_else(|| ServiceError::Auth("user has no password set".into()))?;
        if !self.hasher.verify(password, &hash)? {
            return Err(ServiceError::Auth("unknown user or wrong password".into()));
        }
        if self.hasher.needs_rehash(&hash) {
            let upgraded = self.hasher.hash(password)?;
            sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE name = ?")
                .bind(upgraded)
                .bind(Utc::now().to_rfc3339())
                .bind(name)
                .execute(sqlite(&self.pool)?)
                .await
                .map_err(map_sqlx)?;
        }
        let user = user_from_row(row)?;
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

    async fn find_row(&self, name: &str) -> Result<Option<sqlx::sqlite::SqliteRow>, ServiceError> {
        sqlx::query("SELECT * FROM users WHERE name = ?")
            .bind(name)
            .fetch_optional(sqlite(&self.pool)?)
            .await
            .map_err(map_sqlx)
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

fn user_from_row(row: sqlx::sqlite::SqliteRow) -> Result<User, ServiceError> {
    let name: String = row.get("name");
    let registered_at = parse_dt(
        row.try_get::<Option<String>, _>("registered_at")
            .ok()
            .flatten(),
    );
    let mut user = User::new(name);
    user.metadata.creation_timestamp = parse_dt(
        row.try_get::<Option<String>, _>("created_at")
            .ok()
            .flatten(),
    );
    user.spec = Some(UserSpec {
        display_name: row.get("display_name"),
        email: row.get("email"),
        email_verified: true,
        password: row.try_get("password_hash").ok().flatten(),
        avatar: row.try_get("avatar").ok().flatten(),
        bio: row.try_get("bio").ok().flatten(),
        disabled: Some(row.get::<i64, _>("disabled") != 0),
        registered_at,
        ..UserSpec::default()
    });
    Ok(user)
}

fn parse_dt(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|raw| DateTime::parse_from_rfc3339(&raw).ok())
        .map(|dt| dt.with_timezone(&Utc))
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
