//! Raw temporary file handling for downloads

use std::path::{Path, PathBuf};
use anyhow::Result;

/// Manages raw temporary files for download chunks
pub struct RawTempManager {
    base_dir: PathBuf,
}

impl RawTempManager {
    pub fn new(base_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&base_dir).ok();
        Self { base_dir }
    }

    /// Get temp file path for a chunk
    pub fn chunk_temp_path(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
        chunk_start: i64,
        chunk_end: i64,
    ) -> PathBuf {
        let filename = format!(
            "{}-{}-{}-{}-{}.chunk",
            exchange,
            symbol,
            timeframe,
            chunk_start,
            chunk_end
        );
        self.base_dir.join(filename)
    }

    /// Get part file path for resumable downloads
    pub fn part_file_path(
        &self,
        exchange: &str,
        symbol: &str,
        timeframe: &str,
    ) -> PathBuf {
        let filename = format!(
            "{}-{}-{}.part",
            exchange,
            symbol,
            timeframe
        );
        self.base_dir.join(filename)
    }

    /// Clean up temp files
    pub fn cleanup(&self) -> Result<()> {
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "chunk" || e == "part").unwrap_or(false) {
                std::fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// List all part files (for resume)
    pub fn list_part_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.base_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|e| e == "part").unwrap_or(false))
                    .collect()
            })
            .unwrap_or_default()
    }
}
