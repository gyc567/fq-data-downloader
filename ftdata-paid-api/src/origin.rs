//! Stub "origin" that produces a fake feather file.
//!
//! Real implementation will shell out to `ftdata-cli` (Q9 decision) or call
//! the ftdata-core crates directly. For now, the origin just writes a
//! deterministic synthetic file based on the request hash, so the API can
//! demonstrate the full upload → signed-URL flow.

use std::path::PathBuf;

use ftdata_paid_pricing::PricingRequest;

use crate::error::{ApiError, ApiResult};

/// A request to the origin. Mirrors the shape of a CLI download command.
#[derive(Debug, Clone)]
pub struct OriginRequest {
    pub exchange: String,
    pub pairs: Vec<String>,
    pub timeframes: Vec<String>,
    pub timerange: String,
    pub format: String,
    pub market: String,
}

impl OriginRequest {
    /// Convert this into a `PricingRequest` for the pricing crate.
    /// Row count is computed as a rough estimate based on timerange +
    /// timeframe + pairs (synthetic for the stub).
    pub fn to_pricing_request(&self) -> PricingRequest {
        // Parse timerange like "20230101-20240601" → days.
        let days = parse_timerange_days(&self.timerange).unwrap_or(30) as u64;

        // For each pair × timeframe, estimate rows based on minutes per bar.
        let mut rows = 0u64;
        for _tf in &self.timeframes {
            // Heuristic: 1440 minutes per day for 1m, scaled down for higher.
            // We just use a rough average across timeframes.
            let per_day: u64 = 1440 / self.timeframes.len() as u64;
            rows += (self.pairs.len() as u64) * days * per_day;
        }

        let timeframe = dominant_timeframe(&self.timeframes);
        let market = if self.market.eq_ignore_ascii_case("futures") {
            ftdata_paid_pricing::Market::Futures
        } else {
            ftdata_paid_pricing::Market::Spot
        };

        PricingRequest {
            rows,
            pairs_count: self.pairs.len(),
            timeframe,
            market,
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 0,
        }
    }
}

fn parse_timerange_days(s: &str) -> Option<i64> {
    let (start, end) = s.split_once('-')?;
    let start = chrono::NaiveDate::parse_from_str(start, "%Y%m%d").ok()?;
    let end = chrono::NaiveDate::parse_from_str(end.trim(), "%Y%m%d")
        .ok()
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    Some((end - start).num_days().max(1))
}

fn dominant_timeframe(timeframes: &[String]) -> ftdata_paid_pricing::Timeframe {
    // Pick the highest-resolution (most expensive) timeframe.
    let order = [
        ("1m", ftdata_paid_pricing::Timeframe::M1),
        ("5m", ftdata_paid_pricing::Timeframe::M5),
        ("15m", ftdata_paid_pricing::Timeframe::M15),
        ("1h", ftdata_paid_pricing::Timeframe::H1),
        ("4h", ftdata_paid_pricing::Timeframe::H4),
        ("1d", ftdata_paid_pricing::Timeframe::D1),
    ];
    for (name, tf) in order {
        if timeframes.iter().any(|t| t == name) {
            return tf;
        }
    }
    ftdata_paid_pricing::Timeframe::M1
}

/// Run the stub origin. Returns the synthesized file path.
pub async fn run(req: &OriginRequest) -> ApiResult<PathBuf> {
    let pricing_req = req.to_pricing_request();
    let _quote = ftdata_paid_pricing::price_quote(&pricing_req)
        .map_err(|e| ApiError::Internal(format!("pricing failed: {e}")))?;

    // Synthetic content: write a small file with a stable hash.
    let tmp = std::env::temp_dir().join(format!(
        "ftdata-paid-origin-{}.bin",
        blake3_like_hash(&format!("{:?}", req))
    ));
    tokio::fs::write(&tmp, b"ftdata-paid stub origin output\n")
        .await
        .map_err(|e| ApiError::Internal(format!("write failed: {e}")))?;
    Ok(tmp)
}

fn blake3_like_hash(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Compute blake3 hash of a file. Stub equivalent of a sha256 fingerprint
/// for the origin output.
pub fn blake3_of_file(path: &str) -> String {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return String::new(),
        };
        hasher.update(&buf[..n]);
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req() -> OriginRequest {
        OriginRequest {
            exchange: "binance".into(),
            pairs: vec!["BTC/USDT".into(), "ETH/USDT".into()],
            timeframes: vec!["1m".into()],
            timerange: "20230101-20240601".into(),
            format: "feather".into(),
            market: "spot".into(),
        }
    }

    #[test]
    fn pricing_request_for_sample_has_two_pairs() {
        let pr = sample_req().to_pricing_request();
        assert_eq!(pr.pairs_count, 2);
        // 2023-01-01 to 2024-06-01 = 517 days, 2 pairs × 517 × 1440 = 1_488_960
        assert_eq!(pr.rows, 2 * 517 * 1440);
    }

    #[test]
    fn dominant_timeframe_picks_highest_resolution() {
        assert_eq!(
            dominant_timeframe(&["1d".into(), "1m".into(), "5m".into()]),
            ftdata_paid_pricing::Timeframe::M1
        );
        assert_eq!(
            dominant_timeframe(&["1h".into(), "4h".into()]),
            ftdata_paid_pricing::Timeframe::H1
        );
        assert_eq!(
            dominant_timeframe(&["4h".into()]),
            ftdata_paid_pricing::Timeframe::H4
        );
    }

    #[test]
    fn futures_market_routes_correctly() {
        let mut r = sample_req();
        r.market = "FUTURES".into();
        assert_eq!(r.to_pricing_request().market, ftdata_paid_pricing::Market::Futures);
        r.market = "spot".into();
        assert_eq!(r.to_pricing_request().market, ftdata_paid_pricing::Market::Spot);
    }
}
