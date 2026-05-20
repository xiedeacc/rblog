# rblog Database Schema Inventory

Source: AWS `/usr/local/blog/data`, inspected May 20, 2026.

## Databases

- `/usr/local/blog/data/rblog.db`: current SQLite database, 14 tables.
- `/usr/local/blog/data/rblog.db.bak`: legacy backup SQLite database, 2 tables.

## Current Database: `rblog.db`

### `_sqlx_migrations`
SQLx migration history.

- `version` BIGINT, primary key
- `description` TEXT, not null
- `installed_on` TIMESTAMP, not null, default `CURRENT_TIMESTAMP`
- `success` BOOLEAN, not null
- `checksum` BLOB, not null
- `execution_time` BIGINT, not null

### `attachments`
Attachment metadata for uploaded objects.

- `key` TEXT, primary key
- `url` TEXT
- `display_name` TEXT
- `media_type` TEXT
- `owner_name` TEXT
- `policy_name` TEXT
- `size` INTEGER, not null, default `0`
- `created_at` TEXT

### `categories`
Post category taxonomy.

- `name` TEXT, primary key
- `display_name` TEXT, not null
- `slug` TEXT, not null
- `description` TEXT
- `cover` TEXT
- `template` TEXT
- `priority` INTEGER, not null, default `0`
- `created_at` TEXT
- `updated_at` TEXT

### `comments`
Top-level comments and replies.

- `name` TEXT, primary key
- `subject_kind` TEXT, not null
- `subject_name` TEXT, not null
- `parent_name` TEXT
- `raw` TEXT, not null
- `html` TEXT, not null
- `owner_kind` TEXT
- `owner_name` TEXT
- `owner_display_name` TEXT
- `owner_email` TEXT
- `owner_website` TEXT
- `user_agent` TEXT
- `ip_address` TEXT
- `approved` INTEGER, not null, default `0`
- `hidden` INTEGER, not null, default `0`
- `top` INTEGER, not null, default `0`
- `priority` INTEGER, not null, default `0`
- `quote_reply` TEXT
- `created_at` TEXT
- `approved_at` TEXT

### `extensions`
Legacy Halo import table kept for migration and recovery.

- `name` TEXT, primary key
- `data` BLOB
- `version` INTEGER

### `menu_items`
Navigation menu items.

- `name` TEXT, primary key
- `menu_name` TEXT
- `display_name` TEXT, not null
- `href` TEXT, not null
- `priority` INTEGER, not null, default `0`
- `parent_name` TEXT
- `created_at` TEXT

### `menus`
Navigation menus.

- `name` TEXT, primary key
- `display_name` TEXT, not null
- `created_at` TEXT

### `pages`
Standalone pages.

- `name` TEXT, primary key
- `title` TEXT, not null
- `slug` TEXT, not null
- `markdown` TEXT, not null, default `''`
- `html` TEXT, not null, default `''`
- `raw_type` TEXT, not null, default `'markdown'`
- `excerpt` TEXT
- `owner` TEXT
- `cover` TEXT
- `template` TEXT
- `published` INTEGER, not null, default `0`
- `visible` TEXT, not null, default `'PUBLIC'`
- `deleted` INTEGER, not null, default `0`
- `pinned` INTEGER, not null, default `0`
- `allow_comment` INTEGER, not null, default `1`
- `priority` INTEGER, not null, default `0`
- `publish_time` TEXT
- `created_at` TEXT
- `updated_at` TEXT
- `deleted_at` TEXT
- `visits` INTEGER, not null, default `0`

### `post_categories`
Post-to-category join table.

- `post_name` TEXT, primary key part
- `category_name` TEXT, primary key part

### `post_tags`
Post-to-tag join table.

- `post_name` TEXT, primary key part
- `tag_name` TEXT, primary key part

### `posts`
Blog posts and rendered content.

- `name` TEXT, primary key
- `title` TEXT, not null
- `slug` TEXT, not null
- `markdown` TEXT, not null, default `''`
- `html` TEXT, not null, default `''`
- `raw_type` TEXT, not null, default `'markdown'`
- `excerpt` TEXT
- `owner` TEXT
- `cover` TEXT
- `template` TEXT
- `published` INTEGER, not null, default `0`
- `visible` TEXT, not null, default `'PUBLIC'`
- `deleted` INTEGER, not null, default `0`
- `pinned` INTEGER, not null, default `0`
- `allow_comment` INTEGER, not null, default `1`
- `priority` INTEGER, not null, default `0`
- `publish_time` TEXT
- `created_at` TEXT
- `updated_at` TEXT
- `deleted_at` TEXT
- `visits` INTEGER, not null, default `0`

### `site_settings`
Flat site/admin settings key-value store.

- `key` TEXT, primary key
- `value` TEXT
- `updated_at` TEXT, not null, default `CURRENT_TIMESTAMP`

### `tags`
Post tag taxonomy.

- `name` TEXT, primary key
- `display_name` TEXT, not null
- `slug` TEXT, not null
- `color` TEXT
- `cover` TEXT
- `created_at` TEXT
- `updated_at` TEXT

### `users`
Admin/user accounts.

- `name` TEXT, primary key
- `display_name` TEXT, not null
- `email` TEXT, not null
- `password_hash` TEXT
- `avatar` TEXT
- `bio` TEXT
- `disabled` INTEGER, not null, default `0`
- `registered_at` TEXT
- `created_at` TEXT
- `updated_at` TEXT

## Backup Database: `rblog.db.bak`

### `_sqlx_migrations`
SQLx migration history.

- `version` BIGINT, primary key
- `description` TEXT, not null
- `installed_on` TIMESTAMP, not null, default `CURRENT_TIMESTAMP`
- `success` BOOLEAN, not null
- `checksum` BLOB, not null
- `execution_time` BIGINT, not null

### `extensions`
Legacy Halo JSON extension table.

- `name` TEXT, primary key
- `data` BLOB
- `version` INTEGER
