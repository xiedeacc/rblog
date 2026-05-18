-- Halo-compatible extensions table, SQLite dialect.
-- rblog ships SQLite support that Halo does not.
-- Note: `version` is nullable to match Halo's `schema-mariadb.sql` exactly,
-- where the JPA `@Version` column does not enforce NOT NULL. rblog never
-- writes a NULL version itself; this is purely for migration compatibility
-- with raw rows imported from a Halo dump.
CREATE TABLE IF NOT EXISTS extensions (
    name    TEXT    NOT NULL PRIMARY KEY,
    data    BLOB,
    version INTEGER
);
