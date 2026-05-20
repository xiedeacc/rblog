//! RSS 2.0 + sitemap.xml + robots.txt.

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use rblog_core::{PostListQuery, PostStatusFilter};

use crate::routes::public::context::{absolute_url, base_context};
use crate::{AppState, HttpError};

/// RSS 2.0 feed of the 20 newest published posts.
pub async fn rss(state: State<AppState>) -> Result<Response, HttpError> {
    let list = state
        .services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Published,
            offset: 0,
            limit: 20,
            public_only: true,
            ..PostListQuery::default()
        })
        .await?;
    let ctx = base_context(&state);
    let site_title = ctx["site"]["title"].as_str().unwrap_or("rblog").to_owned();
    let site_link = absolute_url(&state, "/");
    let mut out = String::with_capacity(8 * 1024);
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(r#"<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">"#);
    out.push_str("\n  <channel>");
    push_tag(&mut out, "title", &site_title);
    push_tag(&mut out, "link", &site_link);
    push_tag(
        &mut out,
        "description",
        ctx["site"]["description"].as_str().unwrap_or("rblog feed"),
    );
    push_tag(
        &mut out,
        "lastBuildDate",
        &Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
    );
    for item in &list.items {
        out.push_str("\n    <item>");
        push_tag(&mut out, "title", &item.title);
        push_tag(&mut out, "link", &absolute_url(&state, &item.permalink));
        push_tag(&mut out, "guid", &absolute_url(&state, &item.permalink));
        if let Some(t) = item.publish_time {
            push_tag(
                &mut out,
                "pubDate",
                &t.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
            );
        }
        if let Some(excerpt) = &item.excerpt {
            push_tag(&mut out, "description", excerpt);
        }
        out.push_str("\n    </item>");
    }
    out.push_str("\n  </channel>");
    out.push_str("\n</rss>\n");

    let mut resp = (out).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/rss+xml; charset=utf-8"),
    );
    Ok(resp)
}

pub async fn sitemap(state: State<AppState>) -> Result<Response, HttpError> {
    let list = state
        .services
        .posts
        .list(PostListQuery {
            status: PostStatusFilter::Published,
            offset: 0,
            limit: 10_000,
            public_only: true,
            ..PostListQuery::default()
        })
        .await?;
    let mut out = String::with_capacity(8 * 1024);
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);
    push_url(&mut out, &absolute_url(&state, "/"), None);
    push_url(&mut out, &absolute_url(&state, "/archives"), None);
    for item in &list.items {
        push_url(
            &mut out,
            &absolute_url(&state, &item.permalink),
            item.publish_time
                .map(|t| t.format("%Y-%m-%d").to_string())
                .as_deref(),
        );
    }
    out.push_str("\n</urlset>\n");
    let mut resp = (out).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml; charset=utf-8"),
    );
    Ok(resp)
}

pub async fn robots(state: State<AppState>) -> Result<Response, HttpError> {
    let body = format!(
        "User-agent: *\nAllow: /\nSitemap: {}\n",
        absolute_url(&state, "/sitemap.xml")
    );
    let mut resp = body.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    Ok(resp)
}

fn push_tag(out: &mut String, name: &str, value: &str) {
    out.push_str("\n    <");
    out.push_str(name);
    out.push('>');
    out.push_str("<![CDATA[");
    out.push_str(value);
    out.push_str("]]>");
    out.push_str("</");
    out.push_str(name);
    out.push('>');
}

fn push_url(out: &mut String, loc: &str, lastmod: Option<&str>) {
    out.push_str("\n  <url>");
    out.push_str("\n    <loc>");
    out.push_str(loc);
    out.push_str("</loc>");
    if let Some(m) = lastmod {
        out.push_str("\n    <lastmod>");
        out.push_str(m);
        out.push_str("</lastmod>");
    }
    out.push_str("\n  </url>");
}
