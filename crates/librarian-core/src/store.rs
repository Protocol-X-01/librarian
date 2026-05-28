use crate::Tool;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;

const SCHEMA: &str = include_str!("schema.sql");

/// SQLite-backed persistence layer. Owns the connection; cheap to construct, not thread-safe
/// (wrap in a Mutex if shared). Designed to be created per-CLI-invocation.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open or create the database at `path` with read+write access. Runs the schema
    /// migration idempotently. Use for `refresh` / `sync` / `dump` — anything that mutates.
    ///
    /// Deliberately uses DELETE (the default) journal mode rather than WAL. WAL would be
    /// slightly faster for concurrent reads but requires write access to the DB's *directory*
    /// to create the `-shm` sidecar file — even for SQLITE_OPEN_READ_ONLY connections —
    /// which breaks the "any user can query /var/lib/librarian/index.db" guarantee.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .with_context(|| format!("opening librarian db at {}", path.display()))?;

        // If the DB was previously WAL (older installs), this conversion checkpoints and
        // switches to DELETE so non-root readers stop needing directory write access.
        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        // One-time migration: drop the old enrichment table if it still uses tool_id PK.
        // The old shape cascade-deleted on every refresh; the new shape is keyed by name
        // and survives. Existing enrichment rows are lost — they were being wiped anyway.
        migrate_enrichment_table(&conn).context("migrating enrichment table")?;

        conn.execute_batch(SCHEMA)
            .context("applying librarian schema")?;

        Ok(Self { conn })
    }

    /// Open the database read-only via `file:…?immutable=1` URI. Non-root users can
    /// query the system-wide DB at /var/lib/librarian/index.db without needing write
    /// access to anything in /var/lib/librarian/.
    ///
    /// `immutable=1` declares the DB will not change while this connection is open,
    /// so SQLite skips lock/sidecar management entirely (no `-wal`, no `-shm`, no
    /// rollback journal). This is correct for our access pattern: writers (pacman hook,
    /// `librarian refresh`) hold the file briefly, readers complete in <100ms.
    /// Cost: a reader running concurrently with a writer may see a transient inconsistency.
    pub fn open_readonly(path: &Path) -> Result<Self> {
        let uri = format!("file:{}?immutable=1", path.to_string_lossy());
        let conn = Connection::open_with_flags(
            std::path::Path::new(&uri),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .with_context(|| format!("opening librarian db read-only at {}", path.display()))?;
        Ok(Self { conn })
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)
            .context("applying librarian schema (in-memory)")?;
        Ok(Self { conn })
    }

    /// Upsert a tool keyed on `(name, source, path)`.
    pub fn upsert_tool(&self, tool: &Tool) -> Result<i64> {
        let path_str = tool.path.as_ref().map(|p| p.to_string_lossy().into_owned());
        let category = tool.category.map(|c| c.as_str().to_string());
        let last_seen = tool.last_seen.to_rfc3339();
        let file_mtime = tool.file_mtime.map(|t| t.to_rfc3339());
        let metadata = serde_json::to_string(&tool.metadata).unwrap_or_else(|_| "null".to_string());

        self.conn.execute(
            r#"
            INSERT INTO tools (name, display_name, path, source, category, version,
                               description, package, last_seen, file_mtime, sha256, metadata)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(name, source, COALESCE(path, '')) DO UPDATE SET
                display_name = excluded.display_name,
                category     = COALESCE(excluded.category, tools.category),
                version      = excluded.version,
                description  = COALESCE(excluded.description, tools.description),
                package      = excluded.package,
                last_seen    = excluded.last_seen,
                file_mtime   = excluded.file_mtime,
                sha256       = excluded.sha256,
                metadata     = excluded.metadata
            "#,
            params![
                tool.name,
                tool.display_name,
                path_str,
                tool.source.as_str(),
                category,
                tool.version,
                tool.description,
                tool.package,
                last_seen,
                file_mtime,
                tool.sha256,
                metadata,
            ],
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Drop all rows for a given source (used at the start of a full rescan).
    pub fn clear_source(&self, source: crate::Source) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM tools WHERE source = ?1", [source.as_str()])?;
        Ok(n)
    }

    /// Total tool count (across all sources).
    pub fn count(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM tools", [], |row| row.get(0))?;
        Ok(n)
    }

    /// Borrow the underlying connection for ad-hoc queries from the CLI layer.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// After a refresh, copy enriched descriptions onto tool rows that lack a description
    /// of their own. This makes the SERP-derived text findable via FTS5 and durable across
    /// refresh cycles (the next collection run reinserts tools without descriptions; this
    /// pass re-fills them from the enrichment table).
    ///
    /// Returns the number of tool rows updated.
    pub fn backfill_descriptions_from_enrichment(&self) -> Result<usize> {
        let n = self.conn.execute(
            r#"
            UPDATE tools SET description = (
                SELECT e.readme_excerpt
                FROM enrichment e
                WHERE e.name = tools.name AND e.readme_excerpt IS NOT NULL
                LIMIT 1
            )
            WHERE (tools.description IS NULL OR tools.description = '')
              AND EXISTS (
                  SELECT 1 FROM enrichment e
                  WHERE e.name = tools.name AND e.readme_excerpt IS NOT NULL
              )
            "#,
            [],
        )?;
        Ok(n)
    }
}

/// Drop the old `enrichment` table if it still has the `tool_id` column (pre-fix schema).
/// SQLite's IF NOT EXISTS won't migrate an existing table with a different shape, so we
/// detect the old shape via pragma_table_info and drop. Idempotent.
fn migrate_enrichment_table(conn: &Connection) -> Result<()> {
    let has_old_column: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('enrichment') WHERE name = 'tool_id'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )
        .unwrap_or(false);

    if has_old_column {
        tracing::info!("migrating enrichment table: dropping old tool_id-based schema");
        conn.execute("DROP TABLE enrichment", [])?;
    }
    Ok(())
}
