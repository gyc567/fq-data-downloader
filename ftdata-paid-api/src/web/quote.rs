//! Quote form page `GET /dashboard/quote`.

use axum::{
    extract::State,
    response::Html,
};
use maud::html;

use crate::state::AppState;

use super::dashboard::shell;

pub async fn form(State(_state): State<AppState>) -> Html<String> {
    let markup = shell("Request a Quote — ftdata-paid", html! {
        h1 { "Request a Quote" }
        p { "Fill out the form below to get a price estimate for your data request." }

        form method="post" action="/dashboard/quote" {
            label for="exchange" { "Exchange" }
            select name="exchange" id="exchange" {
                option value="binance" selected { "Binance" }
            }

            label for="pairs" { "Trading Pairs" }
            textarea name="pairs" id="pairs" placeholder="BTC/USDT, ETH/USDT, BNB/USDT" {
                "BTC/USDT, ETH/USDT"
            }
            p style="font-size:0.85rem;color:#666;margin-top:0.25rem;" { "Comma-separated list of trading pairs." }

            label { "Timeframes" }
            .checkbox-group {
                input type="checkbox" name="timeframes" value="1m";
                label for="timeframes" style="display:inline;" { "1m" }
                input type="checkbox" name="timeframes" value="5m";
                label for="timeframes" style="display:inline;" { "5m" }
                input type="checkbox" name="timeframes" value="15m";
                label for="timeframes" style="display:inline;" { "15m" }
                input type="checkbox" name="timeframes" value="1h";
                label for="timeframes" style="display:inline;" { "1h" }
                input type="checkbox" name="timeframes" value="4h";
                label for="timeframes" style="display:inline;" { "4h" }
                input type="checkbox" name="timeframes" value="1d" checked;
                label for="timeframes" style="display:inline;" { "1d" }
            }

            label for="timerange" { "Time Range" }
            input type="text" name="timerange" id="timerange" value="20230101-20240601" placeholder="YYYYMMDD-YYYYMMDD";

            label for="market" { "Market" }
            select name="market" id="market" {
                option value="spot" selected { "Spot" }
                option value="futures" { "Futures" }
            }

            label for="format" { "Format" }
            select name="format" id="format" {
                option value="feather" selected { "Feather" }
                option value="parquet" { "Parquet" }
                option value="json" { "JSON" }
            }

            button type="submit" { "Get Quote" }
        }

        p style="margin-top:1rem;" {
            "Or use the API directly: "
            code { "POST /v1/quote" }
        }
    });

    Html(markup.into_string())
}
