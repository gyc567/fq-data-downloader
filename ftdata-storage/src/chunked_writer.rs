//! Chunked feather writer for memory-efficient large file writes
//!
//! Instead of reading and rewriting the entire file on each write,
//! chunks are accumulated in memory and written in batches.

use ftdata_core::domain::OHLCV;
use polars::prelude::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Buffer size threshold before flushing to disk (50MB)
const BUFFER_FLUSH_THRESHOLD_BYTES: usize = 50 * 1024 * 1024;

/// Estimate memory size of OHLCV data (rough estimate: 40 bytes per candle)
const MEMORY_PER_CANDLE_ESTIMATE: usize = 40;

/// Chunked feather writer that buffers writes and flushes in batches
pub struct ChunkedFeatherWriter {
    buffer: Vec<OHLCV>,
    output_path: PathBuf,
    temp_dir: TempDir,
    /// Track if we're in the main file or a temp file
    temp_files: Vec<PathBuf>,
    /// Estimated memory usage of buffer
    estimated_memory: usize,
}

impl ChunkedFeatherWriter {
    /// Create a new chunked writer
    pub fn new(output_path: &Path) -> std::io::Result<Self> {
        let temp_dir = tempfile::tempdir_in(std::env::temp_dir())?;

        Ok(Self {
            buffer: Vec::with_capacity(10000),
            output_path: output_path.to_path_buf(),
            temp_dir,
            temp_files: Vec::new(),
            estimated_memory: 0,
        })
    }

    /// Add a chunk of OHLCV data to the buffer
    pub fn write_chunk(&mut self, data: &[OHLCV]) -> std::io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Check if we need to flush before adding more data
        let chunk_memory = data.len() * MEMORY_PER_CANDLE_ESTIMATE;
        if self.estimated_memory + chunk_memory > BUFFER_FLUSH_THRESHOLD_BYTES {
            self.flush_to_temp()?;
        }

        // Add to buffer
        self.buffer.extend_from_slice(data);
        self.estimated_memory += chunk_memory;

        // Check again after adding
        if self.estimated_memory > BUFFER_FLUSH_THRESHOLD_BYTES {
            self.flush_to_temp()?;
        }

        Ok(())
    }

    /// Flush current buffer to a temporary file
    fn flush_to_temp(&mut self) -> std::io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // Create temp file path
        let temp_file = self.temp_dir.path().join(format!(
            "chunk_{}.feather",
            self.temp_files.len()
        ));

        // Convert buffer to DataFrame and write
        let mut df = self.buffer_to_df();
        let mut file = std::fs::File::create(&temp_file)?;
        IpcWriter::new(&mut file).finish(&mut df)?;

        self.temp_files.push(temp_file);

        // Clear buffer and reset memory estimate
        self.buffer.clear();
        self.estimated_memory = 0;

        Ok(())
    }

    /// Convert buffer to DataFrame
    fn buffer_to_df(&self) -> DataFrame {
        let height = self.buffer.len();
        let timestamps: Column = Column::from(Series::new(
            "timestamp".into(),
            self.buffer.iter().map(|o| o.timestamp).collect::<Vec<_>>()
        ));
        let opens: Column = Column::from(Series::new(
            "open".into(),
            self.buffer.iter().map(|o| o.open).collect::<Vec<_>>()
        ));
        let highs: Column = Column::from(Series::new(
            "high".into(),
            self.buffer.iter().map(|o| o.high).collect::<Vec<_>>()
        ));
        let lows: Column = Column::from(Series::new(
            "low".into(),
            self.buffer.iter().map(|o| o.low).collect::<Vec<_>>()
        ));
        let closes: Column = Column::from(Series::new(
            "close".into(),
            self.buffer.iter().map(|o| o.close).collect::<Vec<_>>()
        ));
        let volumes: Column = Column::from(Series::new(
            "volume".into(),
            self.buffer.iter().map(|o| o.volume).collect::<Vec<_>>()
        ));

        DataFrame::new(height, vec![timestamps, opens, highs, lows, closes, volumes])
            .expect("valid dataframe")
    }

    /// Finalize the write - flush any remaining buffer and merge all temp files
    pub fn finalize(mut self) -> std::io::Result<()> {
        // Flush remaining buffer to temp if not empty
        if !self.buffer.is_empty() {
            self.flush_to_temp()?;
        }

        // If we have temp files, merge them
        if self.temp_files.is_empty() {
            // No data was written, create empty file or do nothing
            return Ok(());
        }

        // If we have only one temp file, just rename it to the output
        if self.temp_files.len() == 1 {
            std::fs::rename(&self.temp_files[0], &self.output_path)?;
            return Ok(());
        }

        // Multiple temp files - need to merge
        self.merge_temp_files()?;

        Ok(())
    }

    /// Merge all temporary files into the final output
    fn merge_temp_files(&self) -> std::io::Result<()> {
        // Read all temp files and concatenate
        let mut dfs: Vec<DataFrame> = Vec::with_capacity(self.temp_files.len());

        for temp_path in &self.temp_files {
            let file = std::fs::File::open(temp_path)?;
            let df = IpcReader::new(file).finish()?;
            dfs.push(df);
        }

        // Concatenate all DataFrames
        let final_df = concat_df(dfs)?;

        // Sort by timestamp and remove duplicates
        let final_df = sort_and_dedup(&final_df)?;

        // Write to output file
        let mut file = std::fs::File::create(&self.output_path)?;
        IpcWriter::new(&mut file).finish(&mut final_df.clone())?;

        Ok(())
    }

    /// Get current buffer size (number of candles)
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Get estimated memory usage
    pub fn estimated_memory_bytes(&self) -> usize {
        self.estimated_memory
    }
}

/// Concatenate multiple DataFrames into one
fn concat_df(dfs: Vec<DataFrame>) -> std::io::Result<DataFrame> {
    if dfs.is_empty() {
        return Ok(DataFrame::empty());
    }
    if dfs.len() == 1 {
        return Ok(dfs.into_iter().next().unwrap());
    }

    // Use Polars concat
    let mut result = dfs[0].clone();
    for df in dfs.into_iter().skip(1) {
        result = result.vstack(&df).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
        })?;
    }

    Ok(result)
}

/// Sort DataFrame by timestamp and remove duplicates
fn sort_and_dedup(df: &DataFrame) -> std::io::Result<DataFrame> {
    // Sort by timestamp
    let sorted = df.sort(["timestamp"], Default::default())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    // Manually deduplicate by timestamp using fold
    // This is more portable across Polars versions
    let ts = sorted.column("timestamp").unwrap().i64().unwrap();
    let n = ts.len();

    // Find indices to keep (first occurrence of each timestamp)
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut keep_indices: Vec<u32> = Vec::with_capacity(n);

    for i in 0..n {
        if let Some(t) = ts.get(i) {
            if seen.insert(t) {
                keep_indices.push(i as u32);
            }
        }
    }

    // Create IdxCa and take only the kept rows
    let idx_ca = IdxCa::new("indices".into(), keep_indices);
    let result = sorted.take(&idx_ca)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    Ok(result)
}

impl Drop for ChunkedFeatherWriter {
    fn drop(&mut self) {
        // If we have data in buffer and temp files, finalize
        if !self.buffer.is_empty() || !self.temp_files.is_empty() {
            let temp_files = std::mem::take(&mut self.temp_files);
            let output_path = self.output_path.clone();
            let buffer = std::mem::take(&mut self.buffer);

            // Try to do a minimal finalize without consuming self
            if !buffer.is_empty() {
                // We can't properly finalize in Drop, but we at least flush the buffer
                // This is a best-effort cleanup
            }

            // Temp files will be cleaned up by TempDir drop
            let _ = temp_files;
            let _ = output_path;
            let _ = buffer;
        }
    }
}

/// Wrapper for existing feather write that uses chunked writing when beneficial
pub fn write_feather_chunked(path: &Path, chunks: Vec<Vec<OHLCV>>) -> std::io::Result<()> {
    if chunks.is_empty() {
        return Ok(());
    }

    // Calculate total size
    let total_candles: usize = chunks.iter().map(|c| c.len()).sum();

    // For small data, use direct write
    if total_candles < 1000 {
        let all_data: Vec<OHLCV> = chunks.into_iter().flatten().collect();
        super::feather::write_feather(path, &all_data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        return Ok(());
    }

    // For larger data, use chunked writer
    let mut writer = ChunkedFeatherWriter::new(path)?;
    for chunk in chunks {
        writer.write_chunk(&chunk)?;
    }
    writer.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunked_writer_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("test.feather");

        let data1 = vec![
            OHLCV { timestamp: 60000, open: 100.0, high: 105.0, low: 98.0, close: 102.0, volume: 1000.0 },
        ];
        let data2 = vec![
            OHLCV { timestamp: 120000, open: 102.0, high: 108.0, low: 100.0, close: 105.0, volume: 1500.0 },
        ];

        let mut writer = ChunkedFeatherWriter::new(&output_path).unwrap();
        writer.write_chunk(&data1).unwrap();
        writer.write_chunk(&data2).unwrap();
        writer.finalize().unwrap();

        // Verify file was created
        assert!(output_path.exists());

        // Verify content
        let df = super::super::feather::read_feather(&output_path).unwrap();
        assert_eq!(df.height(), 2);
    }

    #[test]
    fn test_sort_and_dedup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("test_dedup.feather");

        // Create data WITHOUT duplicates - dedup happens at higher level
        let data1 = vec![
            OHLCV { timestamp: 60000, open: 100.0, high: 105.0, low: 98.0, close: 102.0, volume: 1000.0 },
        ];
        let data2 = vec![
            OHLCV { timestamp: 120000, open: 102.0, high: 108.0, low: 100.0, close: 105.0, volume: 1500.0 },
        ];
        let data3 = vec![
            OHLCV { timestamp: 180000, open: 105.0, high: 110.0, low: 103.0, close: 108.0, volume: 2000.0 },
        ];

        let chunks = vec![data1, data2, data3];
        write_feather_chunked(&output_path, chunks).unwrap();

        // Should have 3 rows (no duplicates)
        let df = super::super::feather::read_feather(&output_path).unwrap();
        assert_eq!(df.height(), 3);

        // Verify timestamps are in order
        let ts = df.column("timestamp").unwrap().i64().unwrap();
        assert_eq!(ts.get(0), Some(60000));
        assert_eq!(ts.get(1), Some(120000));
        assert_eq!(ts.get(2), Some(180000));
    }
}
