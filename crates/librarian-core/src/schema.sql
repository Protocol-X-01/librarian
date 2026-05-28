-- Librarian SQLite schema. Applied idempotently on every Store::open.

CREATE TABLE IF NOT EXISTS tools (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    name          TEXT NOT NULL,
    display_name  TEXT,
    path          TEXT,
    source        TEXT NOT NULL,
    category      TEXT,
    version       TEXT,
    description   TEXT,
    package       TEXT,
    last_seen     TEXT NOT NULL,
    file_mtime    TEXT,
    sha256        TEXT,
    metadata      TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tools_natural_key
    ON tools(name, source, COALESCE(path, ''));

CREATE INDEX IF NOT EXISTS idx_tools_source   ON tools(source);
CREATE INDEX IF NOT EXISTS idx_tools_category ON tools(category);
CREATE INDEX IF NOT EXISTS idx_tools_name     ON tools(name);

CREATE VIRTUAL TABLE IF NOT EXISTS tools_fts USING fts5(
    name, description, package,
    content='tools',
    content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS tools_fts_ai AFTER INSERT ON tools BEGIN
    INSERT INTO tools_fts(rowid, name, description, package)
    VALUES (new.id, new.name, new.description, new.package);
END;

CREATE TRIGGER IF NOT EXISTS tools_fts_ad AFTER DELETE ON tools BEGIN
    INSERT INTO tools_fts(tools_fts, rowid, name, description, package)
    VALUES ('delete', old.id, old.name, old.description, old.package);
END;

CREATE TRIGGER IF NOT EXISTS tools_fts_au AFTER UPDATE ON tools BEGIN
    INSERT INTO tools_fts(tools_fts, rowid, name, description, package)
    VALUES ('delete', old.id, old.name, old.description, old.package);
    INSERT INTO tools_fts(rowid, name, description, package)
    VALUES (new.id, new.name, new.description, new.package);
END;

CREATE TABLE IF NOT EXISTS sources_state (
    source        TEXT PRIMARY KEY,
    last_scanned  TEXT NOT NULL,
    root_mtime    TEXT,
    tool_count    INTEGER NOT NULL DEFAULT 0,
    last_error    TEXT
);

-- Enrichment is keyed by tool *name*, not row id. Refreshes recycle the `tools` table
-- (each collector clears its source and re-inserts), so an FK to tools.id with CASCADE
-- would wipe the cache on every refresh. A name-based key decouples enrichment from
-- collection cycles: once we've researched "nmap" we never have to research it again
-- until TTL expires, regardless of how many times pacman / aur / yay re-emit the row.
CREATE TABLE IF NOT EXISTS enrichment (
    name           TEXT PRIMARY KEY,
    github_url     TEXT,
    github_stars   INTEGER,
    last_commit    TEXT,
    readme_excerpt TEXT,
    advisories     TEXT,
    raw_serp       TEXT,
    last_fetched   TEXT NOT NULL,
    ttl_seconds    INTEGER NOT NULL DEFAULT 2592000
);

CREATE INDEX IF NOT EXISTS idx_enrichment_last_fetched ON enrichment(last_fetched);
