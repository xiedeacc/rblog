//! Seed values used during first-run bootstrap.
//!
//! On a fresh install rblog ships:
//!
//! - A `super-admin` [`Role`] that grants `*` on every API group / resource /
//!   verb (and `*` on non-resource URLs).
//! - A `RoleBinding` linking the initial admin user to `super-admin`.
//! - The initial admin [`User`] itself, given a password the operator
//!   supplied at first-run (we expect the caller to pass the *hashed*
//!   password; this module does not perform hashing on its own).
//!
//! The first-run flow runs once and is idempotent: if any of these
//! extensions already exist, the bootstrap simply leaves them alone.

use rblog_content::core::{PolicyRule, Role, RoleBinding, RoleRef, Subject, User, UserSpec};

/// Stable name for the system super-admin role.
pub const SUPER_ADMIN_ROLE: &str = "super-admin";
/// Stable label key marking system-reserved RBAC objects so the admin UI can
/// flag them as non-editable.
pub const SYSTEM_RESERVED_LABEL: &str = "halo.run/system-reserved";

/// Build the super-admin role.
#[must_use]
pub fn bootstrap_super_admin_role() -> Role {
    let mut role = Role::new(SUPER_ADMIN_ROLE);
    role.metadata.set_label(SYSTEM_RESERVED_LABEL, "true");
    role.rules = Some(vec![PolicyRule {
        api_groups: vec!["*".to_owned()],
        resources: vec!["*".to_owned()],
        resource_names: vec![],
        non_resource_urls: vec!["/*".to_owned()],
        verbs: vec!["*".to_owned()],
    }]);
    role
}

/// Build the role-binding tying `user` to the super-admin role.
#[must_use]
pub fn bootstrap_super_admin_role_binding(user: &str) -> RoleBinding {
    let name = format!("{user}-{SUPER_ADMIN_ROLE}-binding");
    let mut binding = RoleBinding::new(name);
    binding.metadata.set_label(SYSTEM_RESERVED_LABEL, "true");
    binding.subjects = Some(vec![Subject {
        kind: "User".to_owned(),
        name: user.to_owned(),
        api_group: Some(String::new()),
    }]);
    binding.role_ref = Some(RoleRef {
        kind: "Role".to_owned(),
        name: SUPER_ADMIN_ROLE.to_owned(),
        api_group: Some(String::new()),
    });
    binding
}

/// Build a freshly-stamped admin user object (no password set).
///
/// The bootstrap flow expects the caller to set `spec.password` separately
/// with the result of [`crate::PasswordHasher::hash`].
#[must_use]
pub fn bootstrap_admin_user(name: &str, email: &str) -> User {
    User::new(name).with_spec(UserSpec {
        display_name: "Administrator".to_owned(),
        email: email.to_owned(),
        email_verified: true,
        registered_at: Some(chrono::Utc::now()),
        ..UserSpec::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn super_admin_role_grants_star_everything() {
        let r = bootstrap_super_admin_role();
        let rules = r.rules.as_ref().unwrap();
        assert_eq!(rules[0].api_groups, vec!["*"]);
        assert_eq!(rules[0].verbs, vec!["*"]);
        assert_eq!(r.metadata.label(SYSTEM_RESERVED_LABEL), Some("true"));
    }

    #[test]
    fn role_binding_naming_is_deterministic() {
        let b = bootstrap_super_admin_role_binding("alice");
        assert_eq!(b.metadata.name(), "alice-super-admin-binding");
    }

    #[test]
    fn admin_user_email_verified_default() {
        let u = bootstrap_admin_user("admin", "a@b.c");
        let spec = u.spec.as_ref().unwrap();
        assert!(spec.email_verified);
        assert_eq!(spec.display_name, "Administrator");
    }
}
