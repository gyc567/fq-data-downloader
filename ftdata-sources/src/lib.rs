//! Exchange adapters for Binance, Bybit, OKX

use ftdata_core::domain::*;
use ftdata_core::error::DownloadResult;
use async_trait::async_trait;
use chrono::{Datelike, TimeZone};
use std::sync::Arc;

/// Market data source trait for exchange adapters
#[async_trait]
#[async_trait]
pub trait MarketDataSource: Send + Sync {
    fn exchange(&self) -> Exchange;
    fn supported_timeframes(&self) -> Vec<Timeframe>;
    fn supported_market_types(&self) -> Vec<MarketType>;

    async fn get_available_range(
        &self,
        symbol: &Symbol,
        timeframe: &Timeframe,
        market_type: MarketType,
    ) -> DownloadResult<Option<TimeRange>>;

    async fn get_bulk_urls(
        &self,
        symbol: &Symbol,
        timeframe: &Timeframe,
        time_range: &TimeRange,
    ) -> DownloadResult<Vec<DownloadUrl>>;

    async fn fetch_ohlcv(
        &self,
        symbol: &Symbol,
        timeframe: &Timeframe,
        start_time: i64,
        end_time: i64,
        limit: Option<u32>,
    ) -> DownloadResult<Vec<OHLCV>>;
}

/// Binance adapter
pub mod binance {
        use super::*;
        use ftdata_http::rate_limit::TokenBucketLimiter;
        use std::sync::Arc;

        #[derive(Clone)]
        pub struct BinanceSource {
            rate_limiter: Arc<TokenBucketLimiter>,
        }

        impl BinanceSource {
            pub fn new() -> Arc<Self> {
                Arc::new(Self {
                    rate_limiter: TokenBucketLimiter::new(20.0, 8), // 20 req/s, 8 concurrent
                })
            }
        }

        impl Default for BinanceSource {
            fn default() -> Self {
                Self {
                    rate_limiter: TokenBucketLimiter::new(20.0, 8),
                }
            }
        }

        #[async_trait]
        impl MarketDataSource for BinanceSource {
            fn exchange(&self) -> Exchange {
                Exchange::Binance
            }

            fn supported_timeframes(&self) -> Vec<Timeframe> {
                vec![
                    Timeframe::m1(),
                    Timeframe::m5(),
                    Timeframe::m15(),
                    Timeframe::m30(),
                    Timeframe::h1(),
                    Timeframe::h4(),
                    Timeframe::d1(),
                ]
            }

            fn supported_market_types(&self) -> Vec<MarketType> {
                vec![MarketType::Spot, MarketType::Futures]
            }

            async fn get_available_range(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _market_type: MarketType,
            ) -> DownloadResult<Option<TimeRange>> {
                // Binance has data from 2017 onwards
                let start = 1500000000000i64; // ~July 2017
                let end = chrono::Utc::now().timestamp_millis();
                Ok(Some(TimeRange::new(start, end)))
            }

            async fn get_bulk_urls(
                &self,
                symbol: &Symbol,
                timeframe: &Timeframe,
                time_range: &TimeRange,
            ) -> DownloadResult<Vec<DownloadUrl>> {
                let mut urls = vec![];
                let mut current = time_range.start;

                while current < time_range.end {
                    let dt = chrono::Utc.timestamp_millis_opt(current).unwrap();
                    let year = dt.year();
                    let month = dt.month();

                    let url = format!(
                        "https://data.binance.vision/data/spot/monthly/klines/{}/{}/{}-{}-{}-{:02}.zip",
                        symbol.freqtrade_format(),
                        timeframe.label,
                        symbol.freqtrade_format(),
                        timeframe.label,
                        year,
                        month
                    );

                    // Calculate end of this month
                    let (next_year, next_month) = if month == 12 {
                        (year + 1, 1)
                    } else {
                        (year, month + 1)
                    };
                    let next_month_start = chrono::Utc
                        .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                        .unwrap()
                        .timestamp_millis();
                    let chunk_end = time_range.end.min(next_month_start);

                    urls.push(DownloadUrl {
                        url,
                        etag: None,
                        last_modified: None,
                        content_length: None,
                        time_range: TimeRange::new(current, chunk_end),
                    });

                    current = chunk_end;
                }

                Ok(urls)
            }

            async fn fetch_ohlcv(
                &self,
                symbol: &Symbol,
                timeframe: &Timeframe,
                start_time: i64,
                end_time: i64,
                limit: Option<u32>,
            ) -> DownloadResult<Vec<OHLCV>> {
                let limit = limit.unwrap_or(1000);
                let interval = timeframe.label.as_str();

                let url = format!(
                    "https://api.binance.com/api/v3/klines?symbol={}&interval={}&startTime={}&endTime={}&limit={}",
                    symbol.freqtrade_format(),
                    interval,
                    start_time,
                    end_time,
                    limit
                );

                #[derive(serde::Deserialize)]
                struct BinanceKline {
                    #[serde(rename = "0")]
                    open_time: i64,
                    #[serde(rename = "1")]
                    open: String,
                    #[serde(rename = "2")]
                    high: String,
                    #[serde(rename = "3")]
                    low: String,
                    #[serde(rename = "4")]
                    close: String,
                    #[serde(rename = "5")]
                    volume: String,
                    #[serde(rename = "6")]
                    close_time: i64,
                }

                let response = reqwest::get(&url).await?;
                let klines: Vec<BinanceKline> = response.json().await?;

                let ohlcv_data = klines
                    .into_iter()
                    .map(|k| OHLCV {
                        timestamp: k.open_time,
                        open: k.open.parse().unwrap_or(0.0),
                        high: k.high.parse().unwrap_or(0.0),
                        low: k.low.parse().unwrap_or(0.0),
                        close: k.close.parse().unwrap_or(0.0),
                        volume: k.volume.parse().unwrap_or(0.0),
                    })
                    .collect();

                Ok(ohlcv_data)
            }
        }
}

/// Bybit adapter
pub mod bybit {
        use super::*;
        use ftdata_http::rate_limit::TokenBucketLimiter;
        use std::sync::Arc;

        pub struct BybitSource {
            rate_limiter: Arc<TokenBucketLimiter>,
        }

        impl BybitSource {
            pub fn new() -> Arc<Self> {
                Arc::new(Self {
                    rate_limiter: TokenBucketLimiter::new(10.0, 8), // 10 req/s
                })
            }
        }

        #[async_trait]
        impl MarketDataSource for BybitSource {
            fn exchange(&self) -> Exchange {
                Exchange::Bybit
            }

            fn supported_timeframes(&self) -> Vec<Timeframe> {
                vec![
                    Timeframe::m1(),
                    Timeframe::m5(),
                    Timeframe::m15(),
                    Timeframe::m30(),
                    Timeframe::h1(),
                    Timeframe::h4(),
                    Timeframe::d1(),
                ]
            }

            fn supported_market_types(&self) -> Vec<MarketType> {
                vec![MarketType::Spot, MarketType::Futures]
            }

            async fn get_available_range(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _market_type: MarketType,
            ) -> DownloadResult<Option<TimeRange>> {
                // Bybit spot launched later
                let start = 1570000000000i64; // ~October 2019
                let end = chrono::Utc::now().timestamp_millis();
                Ok(Some(TimeRange::new(start, end)))
            }

            async fn get_bulk_urls(
                &self,
                symbol: &Symbol,
                timeframe: &Timeframe,
                time_range: &TimeRange,
            ) -> DownloadResult<Vec<DownloadUrl>> {
                let mut urls = vec![];
                let mut current = time_range.start;

                while current < time_range.end {
                    let dt = chrono::Utc.timestamp_millis_opt(current).unwrap();
                    let year = dt.year();
                    let month = dt.month();
                    let day = dt.day();

                    let url = format!(
                        "https://raw.githubusercontent.com/bybit-exchange/bybit-archive/main/spot/1m/{}/{}-{}-{}-{}-{:02}.zip",
                        symbol.freqtrade_format(),
                        symbol.freqtrade_format(),
                        timeframe.label,
                        year,
                        month,
                        day
                    );

                    let next_day_start = current + 86_400_000;
                    let chunk_end = time_range.end.min(next_day_start);

                    urls.push(DownloadUrl {
                        url,
                        etag: None,
                        last_modified: None,
                        content_length: None,
                        time_range: TimeRange::new(current, chunk_end),
                    });

                    current = chunk_end;
                }

                Ok(urls)
            }

            async fn fetch_ohlcv(
                &self,
                symbol: &Symbol,
                timeframe: &Timeframe,
                start_time: i64,
                end_time: i64,
                limit: Option<u32>,
            ) -> DownloadResult<Vec<OHLCV>> {
                let limit = limit.unwrap_or(200);
                let interval = timeframe.label.as_str();

                let url = format!(
                    "https://api.bybit.com/v5/market/kline?category=spot&symbol={}&interval={}&start={}&end={}&limit={}",
                    symbol.freqtrade_format(),
                    interval,
                    start_time,
                    end_time,
                    limit
                );

                #[derive(serde::Deserialize)]
                struct BybitKline {
                    list: Vec<String>,
                }

                let response = reqwest::get(&url).await?;
                let data: BybitKline = response.json().await?;

                let ohlcv_data = data
                    .list
                    .into_iter()
                    .map(|item| {
                        let parts: Vec<&str> = item.split(',').collect();
                        OHLCV {
                            timestamp: parts[0].parse().unwrap_or(0),
                            open: parts[1].parse().unwrap_or(0.0),
                            high: parts[2].parse().unwrap_or(0.0),
                            low: parts[3].parse().unwrap_or(0.0),
                            close: parts[4].parse().unwrap_or(0.0),
                            volume: parts[5].parse().unwrap_or(0.0),
                        }
                    })
                    .collect();

                Ok(ohlcv_data)
            }
        }
}

/// OKX adapter
pub mod okx {
        use super::*;
        use ftdata_http::rate_limit::TokenBucketLimiter;
        use std::sync::Arc;

        pub struct OkxSource {
            rate_limiter: Arc<TokenBucketLimiter>,
        }

        impl OkxSource {
            pub fn new() -> Arc<Self> {
                Arc::new(Self {
                    rate_limiter: TokenBucketLimiter::new(10.0, 8), // 10 req/s
                })
            }
        }

        #[async_trait]
        impl MarketDataSource for OkxSource {
            fn exchange(&self) -> Exchange {
                Exchange::OKX
            }

            fn supported_timeframes(&self) -> Vec<Timeframe> {
                vec![
                    Timeframe::m1(),
                    Timeframe::m5(),
                    Timeframe::m15(),
                    Timeframe::m30(),
                    Timeframe::h1(),
                    Timeframe::h4(),
                    Timeframe::d1(),
                ]
            }

            fn supported_market_types(&self) -> Vec<MarketType> {
                vec![MarketType::Spot, MarketType::Futures]
            }

            async fn get_available_range(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _market_type: MarketType,
            ) -> DownloadResult<Option<TimeRange>> {
                let start = 1546300800000i64; // Jan 2019
                let end = chrono::Utc::now().timestamp_millis();
                Ok(Some(TimeRange::new(start, end)))
            }

            async fn get_bulk_urls(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _time_range: &TimeRange,
            ) -> DownloadResult<Vec<DownloadUrl>> {
                // OKX doesn't have public bulk archives like Binance/Bybit
                // Return empty - will fall back to REST API
                Ok(vec![])
            }

            async fn fetch_ohlcv(
                &self,
                symbol: &Symbol,
                timeframe: &Timeframe,
                start_time: i64,
                end_time: i64,
                limit: Option<u32>,
            ) -> DownloadResult<Vec<OHLCV>> {
                let limit = limit.unwrap_or(100);
                let bar = timeframe.label.as_str();

                let url = format!(
                    "https://www.okx.com/api/v5/market/history-candle?instId={}&bar={}&after={}&before={}&limit={}",
                    symbol.freqtrade_format(),
                    bar,
                    end_time,
                    start_time,
                    limit
                );

                #[derive(serde::Deserialize)]
                struct OkxCandle {
                    #[serde(rename = "0")]
                    timestamp: String,
                    #[serde(rename = "1")]
                    open: String,
                    #[serde(rename = "2")]
                    high: String,
                    #[serde(rename = "3")]
                    low: String,
                    #[serde(rename = "4")]
                    close: String,
                    #[serde(rename = "5")]
                    volume: String,
                }

                #[derive(serde::Deserialize)]
                struct OkxData {
                    data: Vec<OkxCandle>,
                }

                let response = reqwest::get(&url).await?;
                let data: OkxData = response.json().await?;

                let ohlcv_data = data
                    .data
                    .into_iter()
                    .map(|c| OHLCV {
                        timestamp: c.timestamp.parse().unwrap_or(0),
                        open: c.open.parse().unwrap_or(0.0),
                        high: c.high.parse().unwrap_or(0.0),
                        low: c.low.parse().unwrap_or(0.0),
                        close: c.close.parse().unwrap_or(0.0),
                        volume: c.volume.parse().unwrap_or(0.0),
                    })
                    .collect();

                Ok(ohlcv_data)
            }
        }
}

/// Generic HTTP source for archives
pub mod generic {
        use super::*;
        use ftdata_http::rate_limit::TokenBucketLimiter;
        use std::sync::Arc;

        pub struct GenericHttpSource {
            rate_limiter: Arc<TokenBucketLimiter>,
        }

        impl GenericHttpSource {
            pub fn new() -> Arc<Self> {
                Arc::new(Self {
                    rate_limiter: TokenBucketLimiter::new(10.0, 4),
                })
            }
        }

        #[async_trait]
        impl MarketDataSource for GenericHttpSource {
            fn exchange(&self) -> Exchange {
                panic!("Generic source doesn't represent a specific exchange")
            }

            fn supported_timeframes(&self) -> Vec<Timeframe> {
                vec![Timeframe::m1(), Timeframe::m5(), Timeframe::m15(), Timeframe::m30(), Timeframe::h1()]
            }

            fn supported_market_types(&self) -> Vec<MarketType> {
                vec![MarketType::Spot]
            }

            async fn get_available_range(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _market_type: MarketType,
            ) -> DownloadResult<Option<TimeRange>> {
                Ok(None)
            }

            async fn get_bulk_urls(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _time_range: &TimeRange,
            ) -> DownloadResult<Vec<DownloadUrl>> {
                Ok(vec![])
            }

            async fn fetch_ohlcv(
                &self,
                _symbol: &Symbol,
                _timeframe: &Timeframe,
                _start_time: i64,
                _end_time: i64,
                _limit: Option<u32>,
            ) -> DownloadResult<Vec<OHLCV>> {
                Ok(vec![])
            }
        }
}

/// Factory for creating exchange adapters
pub struct ExchangeAdapterFactory;

impl ExchangeAdapterFactory {
    pub fn create(exchange: Exchange) -> Arc<dyn MarketDataSource> {
        match exchange {
            Exchange::Binance => binance::BinanceSource::new(),
            Exchange::Bybit => bybit::BybitSource::new(),
            Exchange::OKX => okx::OkxSource::new(),
        }
    }
}
