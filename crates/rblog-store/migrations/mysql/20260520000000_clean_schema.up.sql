-- Clean rblog schema, MySQL dialect.
--
-- MySQL remains build-compatible for now, but the refactor branch targets the
-- live SQLite deployment first. This migration creates the relational shape;
-- data backfill is handled by the SQLite migration used in production.

CREATE TABLE IF NOT EXISTS site_settings (
    `key`      VARCHAR(255) NOT NULL PRIMARY KEY,
    `value`    LONGTEXT,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS users (
    name          VARCHAR(255) NOT NULL PRIMARY KEY,
    display_name  VARCHAR(255) NOT NULL,
    email         VARCHAR(255) NOT NULL UNIQUE,
    password_hash LONGTEXT,
    avatar        LONGTEXT,
    bio           LONGTEXT,
    disabled      BOOLEAN NOT NULL DEFAULT FALSE,
    registered_at DATETIME(3),
    created_at    DATETIME(3),
    updated_at    DATETIME(3)
);

CREATE TABLE IF NOT EXISTS posts (
    name           VARCHAR(255) NOT NULL PRIMARY KEY,
    title          LONGTEXT NOT NULL,
    slug           VARCHAR(255) NOT NULL UNIQUE,
    markdown       LONGTEXT NOT NULL,
    html           LONGTEXT NOT NULL,
    raw_type       VARCHAR(64) NOT NULL DEFAULT 'markdown',
    excerpt        LONGTEXT,
    owner          VARCHAR(255),
    cover          LONGTEXT,
    template       VARCHAR(255),
    published      BOOLEAN NOT NULL DEFAULT FALSE,
    visible        VARCHAR(32) NOT NULL DEFAULT 'PUBLIC',
    deleted        BOOLEAN NOT NULL DEFAULT FALSE,
    pinned         BOOLEAN NOT NULL DEFAULT FALSE,
    allow_comment  BOOLEAN NOT NULL DEFAULT TRUE,
    priority       INT NOT NULL DEFAULT 0,
    publish_time   DATETIME(3),
    created_at     DATETIME(3),
    updated_at     DATETIME(3),
    deleted_at     DATETIME(3),
    visits         BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS pages LIKE posts;

CREATE TABLE IF NOT EXISTS tags (
    name         VARCHAR(255) NOT NULL PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    slug         VARCHAR(255) NOT NULL UNIQUE,
    color        VARCHAR(64),
    cover        LONGTEXT,
    created_at   DATETIME(3),
    updated_at   DATETIME(3)
);

CREATE TABLE IF NOT EXISTS categories (
    name         VARCHAR(255) NOT NULL PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    slug         VARCHAR(255) NOT NULL UNIQUE,
    description  LONGTEXT,
    cover        LONGTEXT,
    template     VARCHAR(255),
    priority     INT NOT NULL DEFAULT 0,
    created_at   DATETIME(3),
    updated_at   DATETIME(3)
);

CREATE TABLE IF NOT EXISTS post_tags (
    post_name VARCHAR(255) NOT NULL,
    tag_name  VARCHAR(255) NOT NULL,
    PRIMARY KEY (post_name, tag_name)
);

CREATE TABLE IF NOT EXISTS post_categories (
    post_name     VARCHAR(255) NOT NULL,
    category_name VARCHAR(255) NOT NULL,
    PRIMARY KEY (post_name, category_name)
);

CREATE TABLE IF NOT EXISTS comments (
    name               VARCHAR(255) NOT NULL PRIMARY KEY,
    subject_kind       VARCHAR(64) NOT NULL,
    subject_name       VARCHAR(255) NOT NULL,
    parent_name        VARCHAR(255),
    raw                LONGTEXT NOT NULL,
    html               LONGTEXT NOT NULL,
    owner_kind         VARCHAR(64),
    owner_name         VARCHAR(255),
    owner_display_name VARCHAR(255),
    owner_email        VARCHAR(255),
    owner_website      LONGTEXT,
    user_agent         LONGTEXT,
    ip_address         VARCHAR(255),
    approved           BOOLEAN NOT NULL DEFAULT FALSE,
    hidden             BOOLEAN NOT NULL DEFAULT FALSE,
    top                BOOLEAN NOT NULL DEFAULT FALSE,
    priority           INT NOT NULL DEFAULT 0,
    quote_reply        VARCHAR(255),
    created_at         DATETIME(3),
    approved_at        DATETIME(3)
);

CREATE TABLE IF NOT EXISTS attachments (
    `key`        VARCHAR(512) NOT NULL PRIMARY KEY,
    url          LONGTEXT,
    display_name VARCHAR(255),
    media_type   VARCHAR(255),
    owner_name   VARCHAR(255),
    policy_name  VARCHAR(255),
    size         BIGINT NOT NULL DEFAULT 0,
    created_at   DATETIME(3)
);

CREATE TABLE IF NOT EXISTS menus (
    name         VARCHAR(255) NOT NULL PRIMARY KEY,
    display_name VARCHAR(255) NOT NULL,
    created_at   DATETIME(3)
);

CREATE TABLE IF NOT EXISTS menu_items (
    name         VARCHAR(255) NOT NULL PRIMARY KEY,
    menu_name    VARCHAR(255),
    display_name VARCHAR(255) NOT NULL,
    href         LONGTEXT NOT NULL,
    priority     INT NOT NULL DEFAULT 0,
    parent_name  VARCHAR(255),
    created_at   DATETIME(3)
);
