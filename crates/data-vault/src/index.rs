/// L0 index.db: SQLite index for Data Vault
///
/// schema:
///   CREATE TABLE entries (
///     id TEXT PRIMARY KEY,
///     ts TEXT NOT NULL,
///     type TEXT NOT NULL,
///     agent_role TEXT,
///     model TEXT,
///     provider TEXT,
///     tokens_prompt INTEGER,
///     tokens_completion INTEGER,
///     session_id TEXT,
///     layer INTEGER DEFAULT 1,
///     file_path TEXT,
///     byte_offset INTEGER,
///     status TEXT DEFAULT 'hot',
///     goal TEXT,
///     result_status TEXT,
///     duration_ms INTEGER,
///     detail_text TEXT
///   );

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// A single index entry in the Data Vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub ts: String,
    pub r#type: String,
    pub agent_role: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub tokens_prompt: Option<i64>,
    pub tokens_completion: Option<i64>,
    pub session_id: Option<String>,
    pub layer: i64,
    pub file_path: Option<String>,
    pub byte_offset: Option<i64>,
    pub status: String,
    pub goal: Option<String>,
    pub result_status: Option<String>,
    pub duration_ms: Option<i64>,
    pub detail_text: Option<String>,
}

/// The Data Vault index, backed by a dedicated SQLite file.
pub struct IndexDb {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl IndexDb {
    /// Open (or create) the index database at the given path.
    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open index db at {}", path.display()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id              TEXT PRIMARY KEY,
                ts              TEXT NOT NULL,
                type            TEXT NOT NULL,
                agent_role      TEXT,
                model           TEXT,
                provider        TEXT,
                tokens_prompt   INTEGER,
                tokens_completion INTEGER,
                session_id      TEXT,
                layer           INTEGER DEFAULT 1,
                file_path       TEXT,
                byte_offset     INTEGER,
                status          TEXT DEFAULT 'hot',
                goal            TEXT,
                result_status   TEXT,
                duration_ms     INTEGER,
                detail_text     TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_entries_ts      ON entries(ts);
            CREATE INDEX IF NOT EXISTS idx_entries_session ON entries(session_id);
            CREATE INDEX IF NOT EXISTS idx_entries_status  ON entries(status);
            CREATE INDEX IF NOT EXISTS idx_entries_layer   ON entries(layer);",
        )
        .context("failed to initialize index db schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Default path: ~/.udas/data-vault/index.db
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".udas")
            .join("data-vault")
            .join("index.db")
    }

    /// Insert one index entry.
    pub fn insert(&self, entry: &IndexEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO entries
             (id, ts, type, agent_role, model, provider,
              tokens_prompt, tokens_completion, session_id,
              layer, file_path, byte_offset, status,
              goal, result_status, duration_ms, detail_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                     ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                entry.id,
                entry.ts,
                entry.r#type,
                entry.agent_role,
                entry.model,
                entry.provider,
                entry.tokens_prompt,
                entry.tokens_completion,
                entry.session_id,
                entry.layer,
                entry.file_path,
                entry.byte_offset,
                entry.status,
                entry.goal,
                entry.result_status,
                entry.duration_ms,
                entry.detail_text,
            ],
        )?;
        Ok(())
    }

    /// Bulk insert many entries in a single transaction.
    pub fn insert_batch(&self, entries: &[IndexEntry]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("BEGIN TRANSACTION")?;
        for entry in entries {
            conn.execute(
                "INSERT OR REPLACE INTO entries ...",
                params![
                    entry.id, entry.ts, entry.r#type, entry.agent_role,
                    entry.model, entry.provider, entry.tokens_prompt,
                    entry.tokens_completion, entry.session_id, entry.layer,
                    entry.file_path, entry.byte_offset, entry.status,
                    entry.goal, entry.result_status, entry.duration_ms,
                    entry.detail_text,
                ],
            )?;
        }
        conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Update the `status` field for a given entry.
    pub fn set_status(&self, id: &str, status: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE entries SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    /// Batch-update status: move all entries matching (session_id, old_status) to new_status.
    pub fn migrate_status(&self, session_id: &str, old_status: &str, new_status: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE entries SET status = ?1 WHERE session_id = ?2 AND status = ?3",
            params![new_status, session_id, old_status],
        )?;
        Ok(n)
    }

    /// Search entries by a text query against detail_text (simple LIKE).
    pub fn search_text(&self, query: &str, limit: i64) -> Result<Vec<IndexEntry>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT * FROM entries WHERE detail_text LIKE ?1 OR id LIKE ?1
             ORDER BY ts DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit], |row| {
            Ok(IndexEntry {
                id: row.get(0)?,
                ts: row.get(1)?,
                r#type: row.get(2)?,
                agent_role: row.get(3)?,
                model: row.get(4)?,
                provider: row.get(5)?,
                tokens_prompt: row.get(6)?,
                tokens_completion: row.get(7)?,
                session_id: row.get(8)?,
                layer: row.get(9)?,
                file_path: row.get(10)?,
                byte_offset: row.get(11)?,
                status: row.get(12)?,
                goal: row.get(13)?,
                result_status: row.get(14)?,
                duration_ms: row.get(15)?,
                detail_text: row.get(16)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// List sessions that have entries in the index.
    pub fn list_sessions(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id FROM entries WHERE session_id IS NOT NULL
             GROUP BY session_id ORDER BY MAX(ts) DESC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count total entries (for health stats).
    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .context("failed to count index entries")
    }

    /// Compress `detail_text` for entries older than `before_ts` to save space.
    pub fn compress_detail(&self, before_ts: &str) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE entries SET detail_text = substring(detail_text, 1, 200)
             WHERE ts < ?1 AND length(detail_text) > 200",
            params![before_ts],
        )?;
        Ok(n)
    }

    /// Rebuild the index by clearing all entries (for recovery after corruption).
    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM entries")?;
        Ok(())
    }
}
