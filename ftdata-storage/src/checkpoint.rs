//! Checkpoint persistence for download resume support
//!
//! Tracks per-chunk download state using SQLite to allow resuming
//! after interruption.

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CheckpointError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Checkpoint status for a chunk
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointStatus {
    Pending,
    Downloading,
    Completed,
    Failed,
}

impl CheckpointStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckpointStatus::Pending => "pending",
            CheckpointStatus::Downloading => "downloading",
            CheckpointStatus::Completed => "completed",
            CheckpointStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(CheckpointStatus::Pending),
            "downloading" => Some(CheckpointStatus::Downloading),
            "completed" => Some(CheckpointStatus::Completed),
            "failed" => Some(CheckpointStatus::Failed),
            _ => None,
        }
    }
}

/// Checkpoint record for a single chunk
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub id: Option<i64>,
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub chunk_url: String,
    pub status: CheckpointStatus,
    pub downloaded_at: Option<i64>,
    pub etag: Option<String>,
}

impl Checkpoint {
    pub fn new(
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        chunk_url: &str,
    ) -> Self {
        Self {
            id: None,
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            timeframe: timeframe.to_string(),
            chunk_url: chunk_url.to_string(),
            status: CheckpointStatus::Pending,
            downloaded_at: None,
            etag: None,
        }
    }
}

/// Checkpoint manager for persistence
pub struct CheckpointManager {
    conn: Arc<Mutex<Connection>>,
}

impl CheckpointManager {
    /// Open or create a checkpoint database at the given path
    pub fn new(db_path: &Path) -> Result<Self, CheckpointError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        let manager = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        manager.init_db()?;
        Ok(manager)
    }

    /// Initialize the checkpoints table
    fn init_db(&self) -> Result<(), CheckpointError> {
        let conn = self.conn.lock();
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS checkpoints (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                exchange        TEXT NOT NULL,
                symbol          TEXT NOT NULL,
                timeframe       TEXT NOT NULL,
                chunk_url       TEXT NOT NULL UNIQUE,
                status          TEXT NOT NULL DEFAULT 'pending',
                downloaded_at   INTEGER,
                etag            TEXT
            )
            "#,
            [],
        )?;

        // Index for fast lookups by exchange/symbol/timeframe
        conn.execute(
            r#"CREATE INDEX IF NOT EXISTS idx_checkpoints_lookup
               ON checkpoints(exchange, symbol, timeframe)"#,
            [],
        )?;

        Ok(())
    }

    /// Save or update a checkpoint
    pub fn save_checkpoint(&self, cp: &Checkpoint) -> Result<i64, CheckpointError> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis();
        let downloaded_at = cp.downloaded_at.unwrap_or(now);

        conn.execute(
            r#"INSERT INTO checkpoints (exchange, symbol, timeframe, chunk_url, status, downloaded_at, etag)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT(chunk_url) DO UPDATE SET
                   status = excluded.status,
                   downloaded_at = excluded.downloaded_at,
                   etag = excluded.etag"#,
            params![
                cp.exchange,
                cp.symbol,
                cp.timeframe,
                cp.chunk_url,
                cp.status.as_str(),
                downloaded_at,
                cp.etag,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get all checkpoints for a given exchange/symbol/timeframe
    pub fn get_checkpoints(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
    ) -> Result<Vec<Checkpoint>, CheckpointError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"SELECT id, exchange, symbol, timeframe, chunk_url, status, downloaded_at, etag
               FROM checkpoints
               WHERE exchange = ?1 AND symbol = ?2 AND timeframe = ?3"#,
        )?;

        let checkpoints = stmt.query_map(params![exchange, symbol, timeframe], |row| {
            Ok(Checkpoint {
                id: row.get(0)?,
                exchange: row.get(1)?,
                symbol: row.get(2)?,
                timeframe: row.get(3)?,
                chunk_url: row.get(4)?,
                status: CheckpointStatus::from_str(&row.get::<_, String>(5)?)
                    .unwrap_or(CheckpointStatus::Pending),
                downloaded_at: row.get(6)?,
                etag: row.get(7)?,
            })
        })?;

        checkpoints.collect::<Result<Vec<_>, _>>().map_err(CheckpointError::from)
    }

    /// Get checkpoint by chunk URL
    pub fn get_checkpoint_by_url(&self, chunk_url: &str) -> Result<Option<Checkpoint>, CheckpointError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"SELECT id, exchange, symbol, timeframe, chunk_url, status, downloaded_at, etag
               FROM checkpoints WHERE chunk_url = ?1"#,
        )?;

        let mut rows = stmt.query(params![chunk_url])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Checkpoint {
                id: row.get(0)?,
                exchange: row.get(1)?,
                symbol: row.get(2)?,
                timeframe: row.get(3)?,
                chunk_url: row.get(4)?,
                status: CheckpointStatus::from_str(&row.get::<_, String>(5)?)
                    .unwrap_or(CheckpointStatus::Pending),
                downloaded_at: row.get(6)?,
                etag: row.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Mark a chunk as completed
    pub fn mark_completed(&self, chunk_url: &str, etag: Option<&str>) -> Result<(), CheckpointError> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE checkpoints SET status = 'completed', downloaded_at = ?1, etag = ?2 WHERE chunk_url = ?3",
            params![now, etag, chunk_url],
        )?;
        Ok(())
    }

    /// Mark a chunk as failed
    pub fn mark_failed(&self, chunk_url: &str) -> Result<(), CheckpointError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE checkpoints SET status = 'failed' WHERE chunk_url = ?1",
            params![chunk_url],
        )?;
        Ok(())
    }

    /// Mark a chunk as downloading
    pub fn mark_downloading(&self, chunk_url: &str) -> Result<(), CheckpointError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE checkpoints SET status = 'downloading' WHERE chunk_url = ?1",
            params![chunk_url],
        )?;
        Ok(())
    }

    /// Clear all checkpoints (optionally filtered by exchange)
    pub fn clear_checkpoints(&self, exchange: Option<&str>) -> Result<u64, CheckpointError> {
        let conn = self.conn.lock();
        let deleted = if let Some(ex) = exchange {
            conn.execute(
                "DELETE FROM checkpoints WHERE exchange = ?1",
                params![ex],
            )?
        } else {
            conn.execute("DELETE FROM checkpoints", [])?
        };
        Ok(deleted as u64)
    }

    /// Clear failed checkpoints (optionally filtered by exchange)
    pub fn clear_failed(&self, exchange: Option<&str>) -> Result<u64, CheckpointError> {
        let conn = self.conn.lock();
        let deleted = if let Some(ex) = exchange {
            conn.execute(
                "DELETE FROM checkpoints WHERE exchange = ?1 AND status = 'failed'",
                params![ex],
            )?
        } else {
            conn.execute(
                "DELETE FROM checkpoints WHERE status = 'failed'",
                [],
            )?
        };
        Ok(deleted as u64)
    }

    /// Get count of checkpoints by status
    pub fn get_status_counts(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
    ) -> Result<StatusCounts, CheckpointError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"SELECT status, COUNT(*) FROM checkpoints
               WHERE exchange = ?1 AND symbol = ?2 AND timeframe = ?3
               GROUP BY status"#,
        )?;

        let mut counts = StatusCounts::default();
        let rows = stmt.query_map(params![exchange, symbol, timeframe], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        for row in rows {
            let (status, count) = row?;
            match status.as_str() {
                "pending" => counts.pending = count as u64,
                "downloading" => counts.downloading = count as u64,
                "completed" => counts.completed = count as u64,
                "failed" => counts.failed = count as u64,
                _ => {}
            }
        }

        Ok(counts)
    }
}

/// Counts of checkpoints by status
#[derive(Debug, Default, Clone)]
pub struct StatusCounts {
    pub pending: u64,
    pub downloading: u64,
    pub completed: u64,
    pub failed: u64,
}

impl StatusCounts {
    pub fn total(&self) -> u64 {
        self.pending + self.downloading + self.completed + self.failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_checkpoint_roundtrip() {
        let db_path = temp_dir().join("test_checkpoint.db");
        let manager = CheckpointManager::new(&db_path).unwrap();

        let cp = Checkpoint::new("binance", "BTC/USDT", "1h", "https://example.com/chunk1.zip");

        let id = manager.save_checkpoint(&cp).unwrap();
        assert!(id > 0);

        let retrieved = manager.get_checkpoint_by_url("https://example.com/chunk1.zip").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.status, CheckpointStatus::Pending);
    }

    #[test]
    fn test_mark_completed() {
        let db_path = temp_dir().join("test_checkpoint2.db");
        let manager = CheckpointManager::new(&db_path).unwrap();

        let cp = Checkpoint::new("binance", "BTC/USDT", "1h", "https://example.com/chunk2.zip");
        manager.save_checkpoint(&cp).unwrap();

        manager.mark_completed("https://example.com/chunk2.zip", Some("etag123")).unwrap();

        let retrieved = manager.get_checkpoint_by_url("https://example.com/chunk2.zip").unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().status, CheckpointStatus::Completed);
    }

    #[test]
    fn test_clear_checkpoints() {
        let db_path = temp_dir().join("test_checkpoint3.db");
        let manager = CheckpointManager::new(&db_path).unwrap();

        for i in 0..5 {
            let cp = Checkpoint::new("binance", "BTC/USDT", "1h", &format!("https://example.com/chunk{}.zip", i));
            manager.save_checkpoint(&cp).unwrap();
        }

        let deleted = manager.clear_checkpoints(Some("binance")).unwrap();
        assert_eq!(deleted, 5);
    }
}
