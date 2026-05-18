//! Role-based access control matching Halo's `RoleService.contains`.
//!
//! Two-step resolution:
//!
//! 1. Find every `RoleBinding` whose `subjects[]` references the given user
//!    (either directly or via group membership; rblog v1 only resolves
//!    user-kind subjects).
//! 2. For each binding's `roleRef`, look up the [`Role`] and walk its
//!    `rules`. A rule matches when *all* of these are true:
//!    - `api_groups` contains the requested group or `*`,
//!    - `resources` contains the requested resource or `*`,
//!    - `resource_names` is empty (means "any") or contains the requested
//!      name or `*`,
//!    - `verbs` contains the requested verb or `*`,
//!    - (or, for a non-resource URL request: `non_resource_urls` matches
//!      and `verbs` permit it).
//!
//! ## Aggregation
//!
//! Unlike Halo, rblog does not yet implement aggregated roles (the
//! `aggregationRule` field in Kubernetes). v1 only walks direct role
//! references.

use rblog_content::core::{PolicyRule, Role, RoleBinding, User};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RbacError {
    #[error("unknown role `{0}` referenced by binding")]
    UnknownRole(String),
}

/// Well-known verb constants matching Kubernetes / Halo conventions.
pub mod verbs {
    pub const GET: &str = "get";
    pub const LIST: &str = "list";
    pub const WATCH: &str = "watch";
    pub const CREATE: &str = "create";
    pub const UPDATE: &str = "update";
    pub const PATCH: &str = "patch";
    pub const DELETE: &str = "delete";
    pub const ALL: &str = "*";
}

/// Typed alias for verb strings to make API signatures more discoverable.
pub type Verb<'a> = &'a str;

/// What the caller wants to do. Either a typed-resource action, or a
/// non-resource URL request (e.g. `/healthz`).
#[derive(Debug, Clone)]
pub struct Attributes<'a> {
    pub api_group: &'a str,
    pub resource: &'a str,
    pub resource_name: Option<&'a str>,
    pub non_resource_url: Option<&'a str>,
    pub verb: Verb<'a>,
}

impl<'a> Attributes<'a> {
    /// Builder for the common case (typed resource).
    #[must_use]
    pub fn resource(api_group: &'a str, resource: &'a str, verb: Verb<'a>) -> Self {
        Self {
            api_group,
            resource,
            resource_name: None,
            non_resource_url: None,
            verb,
        }
    }

    /// Builder for non-resource URLs.
    #[must_use]
    pub fn non_resource(url: &'a str, verb: Verb<'a>) -> Self {
        Self {
            api_group: "",
            resource: "",
            resource_name: None,
            non_resource_url: Some(url),
            verb,
        }
    }

    /// Add a specific resource-name restriction.
    #[must_use]
    pub fn with_name(mut self, name: &'a str) -> Self {
        self.resource_name = Some(name);
        self
    }
}

/// Snapshot of an RBAC ruleset. Cheap to clone; the HTTP layer rebuilds it
/// when the `Role`/`RoleBinding` extensions change.
#[derive(Debug, Clone, Default)]
pub struct AccessChecker {
    roles_by_name: std::collections::HashMap<String, Vec<PolicyRule>>,
    bindings_by_user: std::collections::HashMap<String, Vec<String>>,
}

impl AccessChecker {
    /// Build a checker from the current set of `Role`s and `RoleBinding`s.
    #[must_use]
    pub fn new(roles: &[Role], bindings: &[RoleBinding]) -> Self {
        let mut roles_by_name = std::collections::HashMap::with_capacity(roles.len());
        for role in roles {
            let policy = role.rules.clone().unwrap_or_default();
            roles_by_name.insert(role.metadata.name().to_owned(), policy);
        }

        let mut bindings_by_user: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for b in bindings {
            let Some(role_ref) = &b.role_ref else {
                continue;
            };
            let Some(subjects) = &b.subjects else {
                continue;
            };
            for s in subjects {
                if s.kind == "User" {
                    bindings_by_user
                        .entry(s.name.clone())
                        .or_default()
                        .push(role_ref.name.clone());
                }
            }
        }

        Self {
            roles_by_name,
            bindings_by_user,
        }
    }

    /// Check whether `user` is allowed to perform the action described by
    /// `attrs`. Returns `true` for a permit, `false` otherwise.
    #[must_use]
    pub fn allowed(&self, user: &User, attrs: &Attributes<'_>) -> bool {
        if user.spec.as_ref().is_some_and(|s| s.disabled == Some(true)) {
            return false;
        }
        let Some(role_names) = self.bindings_by_user.get(user.metadata.name()) else {
            return false;
        };
        for role_name in role_names {
            if let Some(rules) = self.roles_by_name.get(role_name) {
                if rules.iter().any(|r| rule_matches(r, attrs)) {
                    return true;
                }
            }
        }
        false
    }

    /// Total number of roles in this snapshot. Useful for liveness checks.
    #[must_use]
    pub fn role_count(&self) -> usize {
        self.roles_by_name.len()
    }

    /// Total number of bound users.
    #[must_use]
    pub fn bound_user_count(&self) -> usize {
        self.bindings_by_user.len()
    }
}

fn rule_matches(rule: &PolicyRule, attrs: &Attributes<'_>) -> bool {
    if let Some(url) = attrs.non_resource_url {
        return matches_any(&rule.non_resource_urls, url) && matches_any(&rule.verbs, attrs.verb);
    }
    matches_any(&rule.api_groups, attrs.api_group)
        && matches_any(&rule.resources, attrs.resource)
        && (rule.resource_names.is_empty()
            || attrs
                .resource_name
                .is_some_and(|n| matches_any(&rule.resource_names, n)))
        && matches_any(&rule.verbs, attrs.verb)
}

fn matches_any(rule_values: &[String], requested: &str) -> bool {
    rule_values
        .iter()
        .any(|v| v == "*" || v == requested || matches_wildcard(v, requested))
}

/// Match Kubernetes-style trailing-`*` wildcards in non-resource URLs:
/// `/api/*` matches `/api/anything`. For resource lists `*` is the only
/// wildcard and is handled directly in [`matches_any`].
fn matches_wildcard(pattern: &str, value: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::{
        bootstrap_admin_user, bootstrap_super_admin_role, bootstrap_super_admin_role_binding,
    };
    use pretty_assertions::assert_eq;
    use rblog_content::core::{PolicyRule, Role, RoleBinding, RoleRef, Subject};

    fn role(name: &str, rules: Vec<PolicyRule>) -> Role {
        let mut r = Role::new(name);
        r.rules = Some(rules);
        r
    }

    fn binding(name: &str, user: &str, role: &str) -> RoleBinding {
        let mut b = RoleBinding::new(name);
        b.subjects = Some(vec![Subject {
            kind: "User".to_owned(),
            name: user.to_owned(),
            api_group: Some(String::new()),
        }]);
        b.role_ref = Some(RoleRef {
            kind: "Role".to_owned(),
            name: role.to_owned(),
            api_group: Some(String::new()),
        });
        b
    }

    fn allow_all() -> PolicyRule {
        PolicyRule {
            api_groups: vec!["*".to_owned()],
            resources: vec!["*".to_owned()],
            resource_names: vec![],
            non_resource_urls: vec!["/*".to_owned()],
            verbs: vec!["*".to_owned()],
        }
    }

    #[test]
    fn super_admin_passes_everything() {
        let roles = [bootstrap_super_admin_role()];
        let bindings = [bootstrap_super_admin_role_binding("admin")];
        let admin = bootstrap_admin_user("admin", "admin@example.com");
        let chk = AccessChecker::new(&roles, &bindings);

        assert!(chk.allowed(
            &admin,
            &Attributes::resource("content.halo.run", "posts", verbs::CREATE),
        ));
        assert!(chk.allowed(&admin, &Attributes::non_resource("/healthz", verbs::GET),));
    }

    #[test]
    fn unbound_user_denied() {
        let roles = [bootstrap_super_admin_role()];
        let bindings: [RoleBinding; 0] = [];
        let admin = bootstrap_admin_user("admin", "admin@example.com");
        let chk = AccessChecker::new(&roles, &bindings);
        assert!(!chk.allowed(
            &admin,
            &Attributes::resource("content.halo.run", "posts", verbs::GET),
        ));
    }

    #[test]
    fn specific_verb_match() {
        let roles = [role(
            "post-reader",
            vec![PolicyRule {
                api_groups: vec!["content.halo.run".to_owned()],
                resources: vec!["posts".to_owned()],
                resource_names: vec![],
                non_resource_urls: vec![],
                verbs: vec![verbs::GET.to_owned(), verbs::LIST.to_owned()],
            }],
        )];
        let bindings = [binding("alice-reader", "alice", "post-reader")];
        let alice = bootstrap_admin_user("alice", "alice@example.com");
        let chk = AccessChecker::new(&roles, &bindings);

        assert!(chk.allowed(
            &alice,
            &Attributes::resource("content.halo.run", "posts", verbs::GET),
        ));
        assert!(!chk.allowed(
            &alice,
            &Attributes::resource("content.halo.run", "posts", verbs::DELETE),
        ));
    }

    #[test]
    fn resource_name_restriction_honored() {
        let roles = [role(
            "post-a-editor",
            vec![PolicyRule {
                api_groups: vec!["content.halo.run".to_owned()],
                resources: vec!["posts".to_owned()],
                resource_names: vec!["post-a".to_owned()],
                non_resource_urls: vec![],
                verbs: vec!["*".to_owned()],
            }],
        )];
        let bindings = [binding("alice-a-editor", "alice", "post-a-editor")];
        let alice = bootstrap_admin_user("alice", "alice@example.com");
        let chk = AccessChecker::new(&roles, &bindings);

        assert!(chk.allowed(
            &alice,
            &Attributes::resource("content.halo.run", "posts", verbs::UPDATE).with_name("post-a"),
        ));
        assert!(!chk.allowed(
            &alice,
            &Attributes::resource("content.halo.run", "posts", verbs::UPDATE).with_name("post-b"),
        ));
        // No name requested but rule has a restriction => deny.
        assert!(!chk.allowed(
            &alice,
            &Attributes::resource("content.halo.run", "posts", verbs::LIST),
        ));
    }

    #[test]
    fn disabled_user_denied_even_when_bound_to_super_admin() {
        let mut admin = bootstrap_admin_user("admin", "admin@example.com");
        admin.spec.as_mut().unwrap().disabled = Some(true);
        let chk = AccessChecker::new(
            &[bootstrap_super_admin_role()],
            &[bootstrap_super_admin_role_binding("admin")],
        );
        assert!(!chk.allowed(
            &admin,
            &Attributes::resource("content.halo.run", "posts", verbs::GET),
        ));
    }

    #[test]
    fn wildcard_non_resource_url() {
        let roles = [role("ops", vec![allow_all()])];
        let bindings = [binding("ops-binding", "ops", "ops")];
        let ops = bootstrap_admin_user("ops", "ops@example.com");
        let chk = AccessChecker::new(&roles, &bindings);
        assert!(chk.allowed(
            &ops,
            &Attributes::non_resource("/anything/at/all", verbs::GET),
        ));
    }

    #[test]
    fn unknown_role_in_binding_is_ignored_silently() {
        // The Halo behaviour is "missing role -> no rules contributed".
        let roles: [Role; 0] = [];
        let bindings = [binding("dangling", "alice", "nonexistent")];
        let alice = bootstrap_admin_user("alice", "alice@example.com");
        let chk = AccessChecker::new(&roles, &bindings);
        assert!(!chk.allowed(&alice, &Attributes::resource("", "users", verbs::GET),));
        assert_eq!(chk.bound_user_count(), 1);
        assert_eq!(chk.role_count(), 0);
    }
}
