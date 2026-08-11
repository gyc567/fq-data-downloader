//! Parquet file handling via Polars

use ftdata_core::domain::OHLCV;
use polars::prelude::*;
use std::path::Path;
use anyhow::Result;

/// Read OHLCV data from parquet file
pub fn read_parquet(path: &Path) -> Result<DataFrame> {
    let file = std::fs::File::open(path)?;
    let df = ParquetReader::new(file).finish()?;
    Ok(df)
}

/// Write OHLCV data to parquet file
pub fn write_parquet(path: &Path, ohlcv_data: &[OHLCV]) -> Result<()> {
    let df = ohlcv_to_df(ohlcv_data);
    let file = std::fs::File::create(path)?;
    ParquetWriter::new(file).finish(&mut df.clone())?;
    Ok(())
}

/// Convert OHLCV slice to Polars DataFrame
pub fn ohlcv_to_df(ohlcv_data: &[OHLCV]) -> DataFrame {
    let height = ohlcv_data.len();
    let timestamps: Column = Column::from(Series::new("timestamp".into(), ohlcv_data.iter().map(|o| o.timestamp).collect::<Vec<_>>()));
    let opens: Column = Column::from(Series::new("open".into(), ohlcv_data.iter().map(|o| o.open).collect::<Vec<_>>()));
    let highs: Column = Column::from(Series::new("high".into(), ohlcv_data.iter().map(|o| o.high).collect::<Vec<_>>()));
    let lows: Column = Column::from(Series::new("low".into(), ohlcv_data.iter().map(|o| o.low).collect::<Vec<_>>()));
    let closes: Column = Column::from(Series::new("close".into(), ohlcv_data.iter().map(|o| o.close).collect::<Vec<_>>()));
    let volumes: Column = Column::from(Series::new("volume".into(), ohlcv_data.iter().map(|o| o.volume).collect::<Vec<_>>()));

    DataFrame::new(height, vec![timestamps, opens, highs, lows, closes, volumes]).expect("valid dataframe")
}

/// Get row count without loading full file
pub fn count_rows(path: &Path) -> Result<i64> {
    let file = std::fs::File::open(path)?;
    let mut reader = ParquetReader::new(file);
    let metadata = reader.get_metadata()?;
    Ok(metadata.num_rows as i64)
}

/// Get file size in bytes
pub fn file_size(path: &Path) -> Result<u64> {
    let metadata = std::fs::metadata(path)?;
    Ok(metadata.len())
}
