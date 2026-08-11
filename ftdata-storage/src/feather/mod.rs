//! Feather file handling via Polars

use ftdata_core::domain::OHLCV;
use polars::prelude::*;
use std::path::Path;
use anyhow::Result;

/// Read OHLCV data from feather file
pub fn read_feather(path: &Path) -> Result<DataFrame> {
    let file = std::fs::File::open(path)?;
    let df = IpcReader::new(file).finish()?;
    Ok(df)
}

/// Write OHLCV data to feather file
pub fn write_feather(path: &Path, ohlcv_data: &[OHLCV]) -> Result<()> {
    let mut df = ohlcv_to_df(ohlcv_data);

    if path.exists() {
        // Read existing, concatenate, rewrite
        let file = std::fs::File::open(path)?;
        let existing = IpcReader::new(file).finish()?;
        df = df.vstack(&existing)?;
    }

    let mut file = std::fs::File::create(path)?;
    IpcWriter::new(&mut file).finish(&mut df)?;
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

/// Convert Polars DataFrame to OHLCV vector
pub fn df_to_ohlcv(df: &DataFrame) -> Vec<OHLCV> {
    let ts = df.column("timestamp").unwrap().i64().unwrap();
    let op = df.column("open").unwrap().f64().unwrap();
    let hi = df.column("high").unwrap().f64().unwrap();
    let lo = df.column("low").unwrap().f64().unwrap();
    let cl = df.column("close").unwrap().f64().unwrap();
    let vo = df.column("volume").unwrap().f64().unwrap();

    (0..df.height()).map(|i| OHLCV {
        timestamp: ts.get(i).unwrap_or(0),
        open: op.get(i).unwrap_or(0.0),
        high: hi.get(i).unwrap_or(0.0),
        low: lo.get(i).unwrap_or(0.0),
        close: cl.get(i).unwrap_or(0.0),
        volume: vo.get(i).unwrap_or(0.0),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ohlcv_to_df() {
        let data = vec![
            OHLCV { timestamp: 60000, open: 100.0, high: 105.0, low: 98.0, close: 102.0, volume: 1000.0 },
            OHLCV { timestamp: 120000, open: 102.0, high: 108.0, low: 100.0, close: 105.0, volume: 1500.0 },
        ];
        let df = ohlcv_to_df(&data);
        assert_eq!(df.height(), 2);
        assert_eq!(df.width(), 6);
    }
}
