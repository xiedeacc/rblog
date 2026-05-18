//! Authentication and authorization primitives.
//!
//! ## What lives here
//!
//! - [`password`] — Argon2id password hashing wrapped in a small type so the
//!   parameters are explicit and not buried under random call sites.
//! - [`session`] — opaque random session token type. The actual session
//!   store (cookies, in-memory, redis) is the HTTP layer's job; this crate
//!   provides the generator and the parser so other code agrees on shape.
//! - [`rbac`] — Halo-compatible RBAC checker. Given an authenticated user,
//!   a set of [`Role`]s and [`RoleBinding`]s, and an attribute set
//!   (`apiGroup`, `resource`, `verb`, `resource_name`), decide whether
//!   access is granted. Mirrors Kubernetes RBAC semantics, including
//!   wildcard `*` matching.
//! - [`seed`] — the system-reserved super-admin role + binding that the
//!   bootstrap flow uses when no `Role`s exist yet.
//!
//! ## What does not live here
//!
//! - HTTP middleware. The axum layer wires `password` and `rbac` into login
//!   endpoints and per-route guards.
//! - Session persistence. Anything that stores a `SessionToken` somewhere
//!   (cookie jar, sqlite, redis) is up to the HTTP layer; we just generate
//!   and recognize the token shape.

pub mod password;
pub mod rbac;
pub mod seed;
pub mod session;

pub use password::{PasswordError, PasswordHasher};
pub use rbac::{AccessChecker, Attributes, RbacError, Verb};
pub use seed::{
    bootstrap_admin_user, bootstrap_super_admin_role, bootstrap_super_admin_role_binding,
    SUPER_ADMIN_ROLE,
};
pub use session::{SessionError, SessionToken};
