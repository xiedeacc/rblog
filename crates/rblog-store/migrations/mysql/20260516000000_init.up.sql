-- Halo-compatible extensions table.
-- IDENTICAL to halo/application/src/main/resources/schema-mariadb.sql so existing
-- Halo databases work unmodified.
CREATE TABLE IF NOT EXISTS extensions (
    name    VARCHAR(255) NOT NULL COLLATE utf8mb4_bin,
    data    longblob,
    version BIGINT,
    PRIMARY KEY (name)
);
