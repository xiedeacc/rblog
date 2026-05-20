//! Shared template context builder.

use axum_extra::extract::cookie::CookieJar;
use serde_json::{json, Value};

use crate::AppState;

/// Build the always-present `{site, menu, active_theme, now}` context.
/// Concrete handlers extend the returned value with page-specific fields.
pub fn base_context(state: &AppState) -> Value {
    let active_theme = state
        .themes
        .active()
        .map_or_else(|_| "default".to_owned(), |t| t.manifest.name.clone());
    let site = site_block(state);
    json!({
        "site": site,
        "menu": Vec::<Value>::new(),
        "active_theme": active_theme,
        "year": chrono::Utc::now().format("%Y").to_string(),
    })
}

pub fn site_context(state: &AppState) -> Value {
    site_block(state)
}

pub async fn current_user(state: &AppState, jar: &CookieJar) -> Option<Value> {
    let cookie = jar.get(&state.config.session.cookie_name)?;
    let session = state.sessions.lookup(cookie.value())?;
    let user = state.services.users.get(&session.user).await.ok()?;
    let spec = user.spec.unwrap_or_default();
    Some(json!({
        "name": user.metadata.name,
        "display_name": spec.display_name,
        "email": spec.email,
    }))
}

fn site_block(state: &AppState) -> Value {
    // Try the configured site.base_url first, then fall back to whatever
    // the bootstrap wrote into the system ConfigMap. Synchronously look at
    // the index for the system ConfigMap so the template never blocks on a
    // database call (the bootstrap already seeded it).
    let cm = state
        .services
        .index
        .get(
            &<rblog_content::core::ConfigMap as rblog_scheme::Extension>::gvk(),
            "system",
        )
        .and_then(|entry| entry.raw.get("data").cloned())
        .unwrap_or(Value::Null);
    let basic = cm
        .get("basic")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let title = cm
        .get("site.title")
        .and_then(Value::as_str)
        .or_else(|| basic.as_ref()?.get("title")?.as_str())
        .map_or_else(|| "rblog".to_owned(), str::to_owned);
    let subtitle = cm
        .get("site.subtitle")
        .and_then(Value::as_str)
        .or_else(|| basic.as_ref()?.get("subtitle")?.as_str())
        .map(str::to_owned);
    let description = cm
        .get("site.description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let configured_base = cm
        .get("site.baseUrl")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let base_url = state.config.site.base_url.clone().or(configured_base);
    let locale = cm
        .get("site.locale")
        .and_then(Value::as_str)
        .map_or_else(|| "en".to_owned(), str::to_owned);
    json!({
        "title": title,
        "subtitle": subtitle,
        "description": description,
        "base_url": base_url,
        "locale": locale,
    })
}

/// Compose the canonical absolute URL for `path`. Used by feed builders.
pub fn absolute_url(state: &AppState, path: &str) -> String {
    let base = state
        .config
        .site
        .base_url
        .clone()
        .or_else(|| {
            state
                .services
                .index
                .get(
                    &<rblog_content::core::ConfigMap as rblog_scheme::Extension>::gvk(),
                    "system",
                )
                .and_then(|entry| {
                    entry
                        .raw
                        .get("data")?
                        .get("site.baseUrl")?
                        .as_str()
                        .map(str::to_owned)
                })
        })
        .unwrap_or_else(|| "http://localhost".to_owned());
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    format!("{base}{path}")
}

/// Compute simple pagination state for a template `_pagination.html`.
pub fn pagination(base_path: &str, page: usize, page_size: usize, total: usize) -> Value {
    let total_pages = total.div_ceil(page_size.max(1)).max(1);
    let page = page.clamp(1, total_pages);
    let prev_url = if page > 1 {
        Some(if page == 2 {
            base_path.to_owned()
        } else {
            format!("/page/{}", page - 1)
        })
    } else {
        None
    };
    let next_url = if page < total_pages {
        Some(format!("/page/{}", page + 1))
    } else {
        None
    };
    json!({
        "page": page,
        "page_size": page_size,
        "total": total,
        "total_pages": total_pages,
        "has_prev": prev_url.is_some(),
        "has_next": next_url.is_some(),
        "prev_url": prev_url,
        "next_url": next_url,
        "base_path": base_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_single_page() {
        let p = pagination("/", 1, 10, 5);
        assert_eq!(p["total_pages"], 1);
        assert_eq!(p["has_prev"], false);
        assert_eq!(p["has_next"], false);
    }

    #[test]
    fn pagination_middle_page() {
        let p = pagination("/", 2, 2, 5);
        assert_eq!(p["total_pages"], 3);
        assert_eq!(p["has_prev"], true);
        assert_eq!(p["has_next"], true);
        assert_eq!(p["prev_url"], "/");
        assert_eq!(p["next_url"], "/page/3");
    }
}
