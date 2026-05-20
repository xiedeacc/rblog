-- Clean rblog schema.
--
-- This migration creates first-class relational tables for the data rblog
-- actually serves. The legacy `extensions` table is intentionally left in
-- place for one migration window so the application can be moved over safely
-- before the final cleanup drops it.

CREATE TABLE IF NOT EXISTS site_settings (
    key        TEXT NOT NULL PRIMARY KEY,
    value      TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS users (
    name          TEXT NOT NULL PRIMARY KEY,
    display_name  TEXT NOT NULL,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT,
    avatar        TEXT,
    bio           TEXT,
    disabled      INTEGER NOT NULL DEFAULT 0,
    registered_at TEXT,
    created_at    TEXT,
    updated_at    TEXT
);

CREATE TABLE IF NOT EXISTS posts (
    name           TEXT NOT NULL PRIMARY KEY,
    title          TEXT NOT NULL,
    slug           TEXT NOT NULL UNIQUE,
    markdown       TEXT NOT NULL DEFAULT '',
    html           TEXT NOT NULL DEFAULT '',
    raw_type       TEXT NOT NULL DEFAULT 'markdown',
    excerpt        TEXT,
    owner          TEXT,
    cover          TEXT,
    template       TEXT,
    published      INTEGER NOT NULL DEFAULT 0,
    visible        TEXT NOT NULL DEFAULT 'PUBLIC',
    deleted        INTEGER NOT NULL DEFAULT 0,
    pinned         INTEGER NOT NULL DEFAULT 0,
    allow_comment  INTEGER NOT NULL DEFAULT 1,
    priority       INTEGER NOT NULL DEFAULT 0,
    publish_time   TEXT,
    created_at     TEXT,
    updated_at     TEXT,
    deleted_at     TEXT,
    visits         INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS pages (
    name           TEXT NOT NULL PRIMARY KEY,
    title          TEXT NOT NULL,
    slug           TEXT NOT NULL UNIQUE,
    markdown       TEXT NOT NULL DEFAULT '',
    html           TEXT NOT NULL DEFAULT '',
    raw_type       TEXT NOT NULL DEFAULT 'markdown',
    excerpt        TEXT,
    owner          TEXT,
    cover          TEXT,
    template       TEXT,
    published      INTEGER NOT NULL DEFAULT 0,
    visible        TEXT NOT NULL DEFAULT 'PUBLIC',
    deleted        INTEGER NOT NULL DEFAULT 0,
    pinned         INTEGER NOT NULL DEFAULT 0,
    allow_comment  INTEGER NOT NULL DEFAULT 1,
    priority       INTEGER NOT NULL DEFAULT 0,
    publish_time   TEXT,
    created_at     TEXT,
    updated_at     TEXT,
    deleted_at     TEXT,
    visits         INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS tags (
    name         TEXT NOT NULL PRIMARY KEY,
    display_name TEXT NOT NULL,
    slug         TEXT NOT NULL UNIQUE,
    color        TEXT,
    cover        TEXT,
    created_at   TEXT,
    updated_at   TEXT
);

CREATE TABLE IF NOT EXISTS categories (
    name         TEXT NOT NULL PRIMARY KEY,
    display_name TEXT NOT NULL,
    slug         TEXT NOT NULL UNIQUE,
    description  TEXT,
    cover        TEXT,
    template     TEXT,
    priority     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT,
    updated_at   TEXT
);

CREATE TABLE IF NOT EXISTS post_tags (
    post_name TEXT NOT NULL,
    tag_name  TEXT NOT NULL,
    PRIMARY KEY (post_name, tag_name),
    FOREIGN KEY (post_name) REFERENCES posts(name) ON DELETE CASCADE,
    FOREIGN KEY (tag_name) REFERENCES tags(name) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS post_categories (
    post_name     TEXT NOT NULL,
    category_name TEXT NOT NULL,
    PRIMARY KEY (post_name, category_name),
    FOREIGN KEY (post_name) REFERENCES posts(name) ON DELETE CASCADE,
    FOREIGN KEY (category_name) REFERENCES categories(name) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS comments (
    name               TEXT NOT NULL PRIMARY KEY,
    subject_kind       TEXT NOT NULL,
    subject_name       TEXT NOT NULL,
    parent_name        TEXT,
    raw                TEXT NOT NULL,
    html               TEXT NOT NULL,
    owner_kind         TEXT,
    owner_name         TEXT,
    owner_display_name TEXT,
    owner_email        TEXT,
    owner_website      TEXT,
    user_agent         TEXT,
    ip_address         TEXT,
    approved           INTEGER NOT NULL DEFAULT 0,
    hidden             INTEGER NOT NULL DEFAULT 0,
    top                INTEGER NOT NULL DEFAULT 0,
    priority           INTEGER NOT NULL DEFAULT 0,
    quote_reply        TEXT,
    created_at         TEXT,
    approved_at        TEXT
);

CREATE TABLE IF NOT EXISTS attachments (
    key          TEXT NOT NULL PRIMARY KEY,
    url          TEXT,
    display_name TEXT,
    media_type   TEXT,
    owner_name   TEXT,
    policy_name  TEXT,
    size         INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT
);

CREATE TABLE IF NOT EXISTS menus (
    name         TEXT NOT NULL PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at   TEXT
);

CREATE TABLE IF NOT EXISTS menu_items (
    name         TEXT NOT NULL PRIMARY KEY,
    menu_name    TEXT,
    display_name TEXT NOT NULL,
    href         TEXT NOT NULL,
    priority     INTEGER NOT NULL DEFAULT 0,
    parent_name  TEXT,
    created_at   TEXT
);

CREATE INDEX IF NOT EXISTS idx_posts_published ON posts(published, deleted, visible, publish_time);
CREATE INDEX IF NOT EXISTS idx_posts_updated ON posts(updated_at);
CREATE INDEX IF NOT EXISTS idx_pages_published ON pages(published, deleted, visible, publish_time);
CREATE INDEX IF NOT EXISTS idx_comments_subject ON comments(subject_kind, subject_name, approved, hidden);
CREATE INDEX IF NOT EXISTS idx_comments_parent ON comments(parent_name);

INSERT OR IGNORE INTO site_settings (key, value)
SELECT je.key, CAST(je.value AS TEXT)
FROM extensions e, json_each(CAST(e.data AS TEXT), '$.data') AS je
WHERE json_extract(CAST(e.data AS TEXT), '$.kind') = 'ConfigMap'
  AND json_extract(CAST(e.data AS TEXT), '$.metadata.name') = 'system';

INSERT OR IGNORE INTO users (
    name, display_name, email, password_hash, avatar, bio, disabled, registered_at, created_at
)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.displayName'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.email'), json_extract(CAST(data AS TEXT), '$.metadata.name') || '@local.invalid'),
    json_extract(CAST(data AS TEXT), '$.spec.password'),
    json_extract(CAST(data AS TEXT), '$.spec.avatar'),
    json_extract(CAST(data AS TEXT), '$.spec.bio'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.disabled'), 0),
    json_extract(CAST(data AS TEXT), '$.spec.registeredAt'),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'User';

INSERT OR IGNORE INTO tags (name, display_name, slug, color, cover, created_at)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.displayName'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.slug'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    json_extract(CAST(data AS TEXT), '$.spec.color'),
    json_extract(CAST(data AS TEXT), '$.spec.cover'),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Tag';

INSERT OR IGNORE INTO categories (name, display_name, slug, description, cover, template, priority, created_at)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.displayName'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.slug'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    json_extract(CAST(data AS TEXT), '$.spec.description'),
    json_extract(CAST(data AS TEXT), '$.spec.cover'),
    json_extract(CAST(data AS TEXT), '$.spec.template'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.priority'), 0),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Category';

INSERT OR IGNORE INTO posts (
    name, title, slug, markdown, html, raw_type, excerpt, owner, cover, template,
    published, visible, deleted, pinned, allow_comment, priority, publish_time,
    created_at, updated_at, deleted_at, visits
)
SELECT
    json_extract(CAST(p.data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.title'), ''),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.slug'), json_extract(CAST(p.data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.rawPatch'), ''),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.contentPatch'), ''),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.rawType'), 'markdown'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.excerpt.raw'), json_extract(CAST(p.data AS TEXT), '$.status.excerpt')),
    json_extract(CAST(p.data AS TEXT), '$.spec.owner'),
    json_extract(CAST(p.data AS TEXT), '$.spec.cover'),
    json_extract(CAST(p.data AS TEXT), '$.spec.template'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.publish'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.visible'), 'PUBLIC'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.deleted'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.pinned'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.allowComment'), 1),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.priority'), 0),
    json_extract(CAST(p.data AS TEXT), '$.spec.publishTime'),
    json_extract(CAST(p.data AS TEXT), '$.metadata.creationTimestamp'),
    json_extract(CAST(p.data AS TEXT), '$.status.lastModifyTime'),
    json_extract(CAST(p.data AS TEXT), '$.metadata.deletionTimestamp'),
    COALESCE(CAST(json_extract(json_extract(json_extract(CAST(p.data AS TEXT), '$.metadata.annotations'), '$."content.halo.run/stats"'), '$.visit') AS INTEGER), 0)
FROM extensions p
LEFT JOIN extensions s
  ON s.name = '/registry/content.halo.run/snapshots/' || COALESCE(
      json_extract(CAST(p.data AS TEXT), '$.spec.headSnapshot'),
      json_extract(CAST(p.data AS TEXT), '$.spec.releaseSnapshot'),
      json_extract(CAST(p.data AS TEXT), '$.spec.baseSnapshot')
  )
WHERE json_extract(CAST(p.data AS TEXT), '$.kind') = 'Post';

INSERT OR IGNORE INTO pages (
    name, title, slug, markdown, html, raw_type, excerpt, owner, cover, template,
    published, visible, deleted, pinned, allow_comment, priority, publish_time,
    created_at, updated_at, deleted_at, visits
)
SELECT
    json_extract(CAST(p.data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.title'), ''),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.slug'), json_extract(CAST(p.data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.rawPatch'), ''),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.contentPatch'), ''),
    COALESCE(json_extract(CAST(s.data AS TEXT), '$.spec.rawType'), 'markdown'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.excerpt.raw'), json_extract(CAST(p.data AS TEXT), '$.status.excerpt')),
    json_extract(CAST(p.data AS TEXT), '$.spec.owner'),
    json_extract(CAST(p.data AS TEXT), '$.spec.cover'),
    json_extract(CAST(p.data AS TEXT), '$.spec.template'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.publish'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.visible'), 'PUBLIC'),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.deleted'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.pinned'), 0),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.allowComment'), 1),
    COALESCE(json_extract(CAST(p.data AS TEXT), '$.spec.priority'), 0),
    json_extract(CAST(p.data AS TEXT), '$.spec.publishTime'),
    json_extract(CAST(p.data AS TEXT), '$.metadata.creationTimestamp'),
    json_extract(CAST(p.data AS TEXT), '$.status.lastModifyTime'),
    json_extract(CAST(p.data AS TEXT), '$.metadata.deletionTimestamp'),
    COALESCE(CAST(json_extract(json_extract(json_extract(CAST(p.data AS TEXT), '$.metadata.annotations'), '$."content.halo.run/stats"'), '$.visit') AS INTEGER), 0)
FROM extensions p
LEFT JOIN extensions s
  ON s.name = '/registry/content.halo.run/snapshots/' || COALESCE(
      json_extract(CAST(p.data AS TEXT), '$.spec.headSnapshot'),
      json_extract(CAST(p.data AS TEXT), '$.spec.releaseSnapshot'),
      json_extract(CAST(p.data AS TEXT), '$.spec.baseSnapshot')
  )
WHERE json_extract(CAST(p.data AS TEXT), '$.kind') = 'SinglePage';

INSERT OR IGNORE INTO post_tags (post_name, tag_name)
SELECT json_extract(CAST(p.data AS TEXT), '$.metadata.name'), jt.value
FROM extensions p, json_each(CAST(p.data AS TEXT), '$.spec.tags') jt
WHERE json_extract(CAST(p.data AS TEXT), '$.kind') = 'Post';

INSERT OR IGNORE INTO post_categories (post_name, category_name)
SELECT json_extract(CAST(p.data AS TEXT), '$.metadata.name'), jc.value
FROM extensions p, json_each(CAST(p.data AS TEXT), '$.spec.categories') jc
WHERE json_extract(CAST(p.data AS TEXT), '$.kind') = 'Post';

INSERT OR IGNORE INTO comments (
    name, subject_kind, subject_name, parent_name, raw, html, owner_kind, owner_name,
    owner_display_name, owner_email, owner_website, user_agent, ip_address, approved,
    hidden, top, priority, quote_reply, created_at, approved_at
)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    json_extract(CAST(data AS TEXT), '$.spec.subjectRef.kind'),
    json_extract(CAST(data AS TEXT), '$.spec.subjectRef.name'),
    NULL,
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.raw'), ''),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.content'), ''),
    json_extract(CAST(data AS TEXT), '$.spec.owner.kind'),
    json_extract(CAST(data AS TEXT), '$.spec.owner.name'),
    json_extract(CAST(data AS TEXT), '$.spec.owner.displayName'),
    json_extract(CAST(data AS TEXT), '$.spec.owner.email'),
    json_extract(CAST(data AS TEXT), '$.spec.owner.website'),
    json_extract(CAST(data AS TEXT), '$.spec.userAgent'),
    json_extract(CAST(data AS TEXT), '$.spec.ipAddress'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.approved'), 0),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.hidden'), 0),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.top'), 0),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.priority'), 0),
    NULL,
    json_extract(CAST(data AS TEXT), '$.spec.creationTime'),
    json_extract(CAST(data AS TEXT), '$.spec.approvedTime')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Comment';

INSERT OR IGNORE INTO attachments (key, url, display_name, media_type, owner_name, policy_name, size, created_at)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    json_extract(CAST(data AS TEXT), '$.status.permalink'),
    json_extract(CAST(data AS TEXT), '$.spec.displayName'),
    json_extract(CAST(data AS TEXT), '$.spec.mediaType'),
    json_extract(CAST(data AS TEXT), '$.spec.ownerName'),
    json_extract(CAST(data AS TEXT), '$.spec.policyName'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.size'), 0),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Attachment';

INSERT OR IGNORE INTO menus (name, display_name, created_at)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.displayName'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Menu';

INSERT OR IGNORE INTO menu_items (name, display_name, href, priority, created_at)
SELECT
    json_extract(CAST(data AS TEXT), '$.metadata.name'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.displayName'), json_extract(CAST(data AS TEXT), '$.metadata.name')),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.href'), json_extract(CAST(data AS TEXT), '$.status.href'), '#'),
    COALESCE(json_extract(CAST(data AS TEXT), '$.spec.priority'), 0),
    json_extract(CAST(data AS TEXT), '$.metadata.creationTimestamp')
FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'MenuItem';
