//! Storage layer for feather/parquet output

pub mod checkpoint;
pub mod feather;
pub mod parquet;
pub mod raw;
pub mod sqlite;

use ftdata_core::domain::*;
use std::path::{Path, PathBuf};

/// Freqtrade-compatible output directory structure
pub struct StorageLayout {
    base_path: PathBuf,
}

impl StorageLayout {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get output path for a symbol/timeframe
    pub fn output_path(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
        timeframe: Timeframe,
        format: DataFormat,
    ) -> PathBuf {
        let exchange_dir = self.base_path.join(exchange.to_string());
        std::fs::create_dir_all(&exchange_dir).ok();
        exchange_dir.join(format!(
            "{}-{}.{}",
            symbol.freqtrade_format(),
            timeframe.label,
            format.extension()
        ))
    }

    /// Get temp path for in-progress downloads
    pub fn temp_path(&self, exchange: Exchange, symbol: &Symbol, timeframe: Timeframe) -> PathBuf {
        let exchange_dir = self.base_path.join("_temp").join(exchange.to_string());
        std::fs::create_dir_all(&exchange_dir).ok();
        exchange_dir.join(format!(
            "{}-{}.part",
            symbol.freqtrade_format(),
            timeframe.label
        ))
    }

    /// Get lock file path
    pub fn lock_path(&self, exchange: Exchange, symbol: &Symbol, timeframe: Timeframe) -> PathBuf {
        let exchange_dir = self.base_path.join("_locks").join(exchange.to_string());
        std::fs::create_dir_all(&exchange_dir).ok();
        exchange_dir.join(format!(
            "{}-{}.lock",
            symbol.freqtrade_format(),
            timeframe.label
        ))
    }

    /// Get manifest path
    pub fn manifest_path(
        &self,
        exchange: Exchange,
        symbol: &Symbol,
        timeframe: Timeframe,
    ) -> PathBuf {
        let exchange_dir = self.base_path.join(exchange.to_string());
        exchange_dir.join(format!(
            "{}-{}.manifest.json",
            symbol.freqtrade_format(),
            timeframe.label
        ))
    }

    /// Get raw temp directory
    pub fn raw_temp_dir(&self, exchange: Exchange) -> PathBuf {
        let dir = self.base_path.join("_raw").join(exchange.to_string());
        std::fs::create_dir_all(&dir).ok();
        dir
    }
}

/// Atomic file commit
pub struct AtomicCommit {
    temp_path: PathBuf,
    final_path: PathBuf,
    lock_path: PathBuf,
}

impl AtomicCommit {
    pub fn new(temp_path: PathBuf, final_path: PathBuf, lock_path: PathBuf) -> Self {
        Self {
            temp_path,
            final_path,
            lock_path,
        }
    }

    /// Acquire lock
    pub async fn acquire_lock(&self) -> Result<(), std::io::Error> {
        tokio::fs::write(&self.lock_path, std::process::id().to_string()).await
    }

    /// Release lock and commit
    pub async fn commit(self) -> Result<(), std::io::Error> {
        // fsync the temp file
        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .open(&self.temp_path)
            .await?;
        file.sync_all().await?;

        // Atomic rename
        tokio::fs::rename(&self.temp_path, &self.final_path).await?;

        // Remove lock
        tokio::fs::remove_file(&self.lock_path).await?;

        Ok(())
    }

    /// Abort and cleanup
    pub async fn abort(self) -> Result<(), std::io::Error> {
        // Remove temp file if exists
        if self.temp_path.exists() {
            tokio::fs::remove_file(&self.temp_path).await?;
        }
        // Remove lock
        if self.lock_path.exists() {
            tokio::fs::remove_file(&self.lock_path).await?;
        }
        Ok(())
    }

    /// Check if lock exists and is stale (process died)
    pub async fn is_lock_stale(&self) -> bool {
        if !self.lock_path.exists() {
            return false;
        }
        // Try to read the PID
        if let Ok(pid_str) = tokio::fs::read_to_string(&self.lock_path).await {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                // Check if process exists (on Unix)
                #[cfg(unix)]
                {
                    use std::process::Command;
                    let output = Command::new("ps")
                        .args(["-p", &pid.to_string()])
                        .output();
                    return output.is_err();
                }
            }
        }
        true
    }
}

/// Dataset manifest for auditing
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DatasetManifest {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub market_type: String,
    pub candle_type: String,
    pub format: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub rows: Option<i64>,
    pub duplicates_removed: i64,
    pub gaps_detected: i64,
    pub checksum: Option<String>,
    pub source: String,
    pub downloaded_at: String,
    pub chunks: Vec<ChunkManifest>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChunkManifest {
    pub range: String,
    pub status: String,
}

impl DatasetManifest {
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        std::fs::write(path, json)
    }

    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })
    }
}
