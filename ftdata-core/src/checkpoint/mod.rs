//! SQLite state management and checkpoint handling

use crate::domain::*;
use crate::error::DownloadError;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Arc;

/// Checkpoint manager for download state
pub struct CheckpointManager {
    conn: Arc<Mutex<Connection>>,
}

impl CheckpointManager {
    /// Create a new checkpoint manager with the given database path
    pub fn new(db_path: &Path) -> Result<Arc<Self>, DownloadError> {
        let conn = Connection::open(db_path)?;
        let manager = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        manager.init_schema()?;
        Ok(Arc::new(manager))
    }

    /// Initialize the database schema
    fn init_schema(&self) -> Result<(), DownloadError> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS downloads (
                id              INTEGER PRIMARY KEY,
                exchange        TEXT NOT NULL,
                market_type     TEXT NOT NULL,
                symbol          TEXT NOT NULL,
                timeframe       TEXT NOT NULL,
                candle_type     TEXT NOT NULL,
                start_ts        INTEGER,
                end_ts          INTEGER,
                status          TEXT NOT NULL,
                source          TEXT NOT NULL,
                bytes_total     INTEGER,
                bytes_downloaded INTEGER,
                checksum        TEXT,
                retry_count     INTEGER DEFAULT 0,
                error           TEXT,
                created_at      INTEGER,
                updated_at      INTEGER
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id              INTEGER PRIMARY KEY,
                download_id     INTEGER REFERENCES downloads(id),
                chunk_start     INTEGER NOT NULL,
                chunk_end       INTEGER NOT NULL,
                status          TEXT NOT NULL,
                size            INTEGER,
                checksum        TEXT,
                etag            TEXT,
                last_modified   TEXT,
                retry_count     INTEGER DEFAULT 0,
                error           TEXT,
                UNIQUE(download_id, chunk_start)
            );

            CREATE TABLE IF NOT EXISTS files (
                path            TEXT PRIMARY KEY,
                exchange        TEXT NOT NULL,
                symbol          TEXT NOT NULL,
                timeframe       TEXT NOT NULL,
                market_type     TEXT NOT NULL,
                candle_type     TEXT NOT NULL,
                from_ts         INTEGER,
                to_ts           INTEGER,
                rows            INTEGER,
                size            INTEGER,
                checksum        TEXT,
                format          TEXT NOT NULL,
                verified        INTEGER DEFAULT 0,
                created_at      INTEGER,
                updated_at      INTEGER
            );

            CREATE TABLE IF NOT EXISTS gaps (
                id              INTEGER PRIMARY KEY,
                exchange        TEXT NOT NULL,
                symbol          TEXT NOT NULL,
                timeframe       TEXT NOT NULL,
                market_type     TEXT NOT NULL,
                candle_type     TEXT NOT NULL,
                from_ts         INTEGER NOT NULL,
                to_ts           INTEGER NOT NULL,
                reason          TEXT,
                status          TEXT DEFAULT 'open',
                created_at      INTEGER
            );

            CREATE INDEX IF NOT EXISTS idx_downloads_exchange_symbol
                ON downloads(exchange, symbol, timeframe);
            CREATE INDEX IF NOT EXISTS idx_chunks_download_id
                ON chunks(download_id);
            CREATE INDEX IF NOT EXISTS idx_files_exchange_symbol
                ON files(exchange, symbol, timeframe);
            CREATE INDEX IF NOT EXISTS idx_gaps_status
                ON gaps(status);
            "#,
        )?;
        Ok(())
    }

    /// Insert a new download record
    pub fn insert_download(&self, download: &Download) -> Result<i64, DownloadError> {
        let conn = self.conn.lock();
        conn.execute(
            r#"INSERT INTO downloads
               (exchange, market_type, symbol, timeframe, candle_type,
                start_ts, end_ts, status, source, bytes_total, bytes_downloaded,
                checksum, retry_count, error, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)"#,
            params![
                download.exchange.to_string(),
                download.market_type.to_string(),
                download.symbol.to_string(),
                download.timeframe.label,
                download.candle_type.to_string(),
                download.start_ts,
                download.end_ts,
                download.status.to_string(),
                download.source.to_string(),
                download.bytes_total,
                download.bytes_downloaded,
                download.checksum,
                download.retry_count,
                download.error,
                download.created_at,
                download.updated_at,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update download status
    pub fn update_download_status(&self, id: i64, status: DownloadStatus) -> Result<(), DownloadError> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE downloads SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), now, id],
        )?;
        Ok(())
    }

    /// Insert a chunk
    pub fn insert_chunk(&self, chunk: &Chunk) -> Result<i64, DownloadError> {
        let conn = self.conn.lock();
        conn.execute(
            r#"INSERT INTO chunks
               (download_id, chunk_start, chunk_end, status, size, checksum,
                etag, last_modified, retry_count, error)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            params![
                chunk.download_id,
                chunk.start,
                chunk.end,
                chunk.status.to_string(),
                chunk.size,
                chunk.checksum,
                chunk.etag,
                chunk.last_modified,
                chunk.retry_count,
                chunk.error,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update chunk status
    pub fn update_chunk_status(
        &self,
        id: i64,
        status: DownloadStatus,
        size: Option<i64>,
        checksum: Option<&str>,
    ) -> Result<(), DownloadError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE chunks SET status = ?1, size = ?2, checksum = ?3 WHERE id = ?4",
            params![status.to_string(), size, checksum, id],
        )?;
        Ok(())
    }

    /// Get pending chunks for a download
    pub fn get_pending_chunks(&self, download_id: i64) -> Result<Vec<Chunk>, DownloadError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, download_id, chunk_start, chunk_end, status, size, checksum,
                    etag, last_modified, retry_count, error
             FROM chunks WHERE download_id = ?1 AND status = 'pending'"
        )?;
        let chunks = stmt.query_map([download_id], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                download_id: row.get(1)?,
                start: row.get(2)?,
                end: row.get(3)?,
                status: row.get::<_, String>(4)?.parse::<DownloadStatus>().unwrap(),
                size: row.get(5)?,
                checksum: row.get(6)?,
                etag: row.get(7)?,
                last_modified: row.get(8)?,
                retry_count: row.get(9)?,
                error: row.get(10)?,
            })
        })?;
        chunks.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Get failed chunks for a download (for retry)
    pub fn get_failed_chunks(&self, download_id: i64) -> Result<Vec<Chunk>, DownloadError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, download_id, chunk_start, chunk_end, status, size, checksum,
                    etag, last_modified, retry_count, error
             FROM chunks WHERE download_id = ?1 AND status = 'failed' AND retry_count < 5"
        )?;
        let chunks = stmt.query_map([download_id], |row| {
            Ok(Chunk {
                id: row.get(0)?,
                download_id: row.get(1)?,
                start: row.get(2)?,
                end: row.get(3)?,
                status: row.get::<_, String>(4)?.parse::<DownloadStatus>().unwrap(),
                size: row.get(5)?,
                checksum: row.get(6)?,
                etag: row.get(7)?,
                last_modified: row.get(8)?,
                retry_count: row.get(9)?,
                error: row.get(10)?,
            })
        })?;
        chunks.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Get download by exchange/symbol/timeframe
    pub fn get_download(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
        timeframe: &Timeframe,
    ) -> Result<Option<Download>, DownloadError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, exchange, market_type, symbol, timeframe, candle_type,
                    start_ts, end_ts, status, source, bytes_total, bytes_downloaded,
                    checksum, retry_count, error, created_at, updated_at
             FROM downloads WHERE exchange = ?1 AND symbol = ?2 AND timeframe = ?3"
        )?;
        let mut rows = stmt.query(params![exchange.to_string(), symbol.to_string(), timeframe.label])?;

        if let Some(row) = rows.next()? {
            Ok(Some(Download {
                id: row.get(0)?,
                exchange: row.get::<_, String>(1)?.parse::<Exchange>().unwrap(),
                market_type: row.get::<_, String>(2)?.parse::<MarketType>().unwrap(),
                symbol: Symbol::parse(&row.get::<_, String>(3)?).unwrap(),
                timeframe: row.get::<_, String>(4)?.parse::<Timeframe>().unwrap(),
                candle_type: row.get::<_, String>(5)?.parse::<CandleType>().unwrap(),
                start_ts: row.get(6)?,
                end_ts: row.get(7)?,
                status: row.get::<_, String>(8)?.parse::<DownloadStatus>().unwrap(),
                source: row.get::<_, String>(9)?.parse::<DownloadSource>().unwrap(),
                bytes_total: row.get(10)?,
                bytes_downloaded: row.get(11)?,
                checksum: row.get(12)?,
                retry_count: row.get(13)?,
                error: row.get(14)?,
                created_at: row.get(15)?,
                updated_at: row.get(16)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Insert or update file metadata
    pub fn upsert_file(&self, meta: &FileMetadata) -> Result<(), DownloadError> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            r#"INSERT INTO files
               (path, exchange, symbol, timeframe, market_type, candle_type,
                from_ts, to_ts, rows, size, checksum, format, verified, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
               ON CONFLICT(path) DO UPDATE SET
                from_ts = excluded.from_ts,
                to_ts = excluded.to_ts,
                rows = excluded.rows,
                size = excluded.size,
                checksum = excluded.checksum,
                verified = excluded.verified,
                updated_at = excluded.updated_at"#,
            params![
                meta.path,
                meta.exchange.to_string(),
                meta.symbol.to_string(),
                meta.timeframe.label,
                meta.market_type.to_string(),
                meta.candle_type.to_string(),
                meta.from_ts,
                meta.to_ts,
                meta.rows,
                meta.size,
                meta.checksum,
                meta.format.to_string(),
                meta.verified as i32,
                meta.created_at.unwrap_or(now),
                now,
            ],
        )?;
        Ok(())
    }

    /// Insert a gap record
    pub fn insert_gap(&self, gap: &Gap) -> Result<i64, DownloadError> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            r#"INSERT INTO gaps
               (exchange, symbol, timeframe, market_type, candle_type,
                from_ts, to_ts, reason, status, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            params![
                gap.exchange.to_string(),
                gap.symbol.to_string(),
                gap.timeframe.label,
                gap.market_type.to_string(),
                gap.candle_type.to_string(),
                gap.from_ts,
                gap.to_ts,
                gap.reason,
                gap.status.to_string(),
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all open gaps
    pub fn get_open_gaps(&self) -> Result<Vec<Gap>, DownloadError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT exchange, symbol, timeframe, market_type, candle_type,
                    from_ts, to_ts, reason, status
             FROM gaps WHERE status = 'open'"
        )?;
        let gaps = stmt.query_map([], |row| {
            Ok(Gap {
                exchange: row.get::<_, String>(0)?.parse::<Exchange>().unwrap(),
                symbol: Symbol::parse(&row.get::<_, String>(1)?).unwrap(),
                timeframe: row.get::<_, String>(2)?.parse::<Timeframe>().unwrap(),
                market_type: row.get::<_, String>(3)?.parse::<MarketType>().unwrap(),
                candle_type: row.get::<_, String>(4)?.parse::<CandleType>().unwrap(),
                from_ts: row.get(5)?,
                to_ts: row.get(6)?,
                reason: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                status: GapStatus::Open,
            })
        })?;
        gaps.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Mark gap as repaired
    pub fn repair_gap(&self, id: i64) -> Result<(), DownloadError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE gaps SET status = 'repaired' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_checkpoint_roundtrip() {
        let db_path = temp_dir().join("test_ftdata.db");
        let manager = CheckpointManager::new(&db_path).unwrap();

        let download = Download::new(
            Exchange::Binance,
            MarketType::Spot,
            Symbol::parse("BTC/USDT").unwrap(),
            Timeframe::m1(),
            CandleType::OHLCV,
            TimeRange::new(0, 1000),
            DownloadSource::Bulk,
        );

        let id = manager.insert_download(&download).unwrap();
        assert!(id > 0);

        let retrieved = manager.get_download(
            Exchange::Binance,
            &Symbol::parse("BTC/USDT").unwrap(),
            &Timeframe::m1(),
        ).unwrap();
        assert!(retrieved.is_some());
    }
}
