//! End-to-end integration tests for the ftdata-paid API.
//!
//! Spins up an Axum server in-process with a `MockFacilitator`, then exercises
//! the full 402 retry flow via `reqwest`.

use std::sync::Arc;
use std::time::Duration;

use axum::serve;
use ftdata_paid_api::{routes::router, AppState};
use ftdata_paid_facilitator::{
    Asset, MockFacilitator, Network, PaymentProof, PaymentVerifier, Scheme, UnixSecs,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Spawn the API on an ephemeral port and return its base URL.
async fn spawn_app() -> (String, Arc<MockFacilitator>) {
    let mock = Arc::new(MockFacilitator::new());
    let state = AppState::new(mock.clone() as Arc<dyn PaymentVerifier>);
    let app = router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        serve(listener, app).await.unwrap();
    });

    // Tiny pause so the listener is ready before the test fires requests.
    tokio::time::sleep(Duration::from_millis(50)).await;
    (format!("http://{addr}"), mock)
}

fn sample_body() -> Value {
    json!({
        "exchange": "binance",
        "pairs": ["BTC/USDT"],
        "timeframes": ["1m"],
        // Q9 fix: use 1-day range instead of 1-month to speed up real-origin download in tests
        "timerange": "20230101-20230102",
        "format": "feather",
        "market": "spot"
    })
}

#[tokio::test]
async fn info_endpoint_returns_metadata() {
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!("{base}/v1/info")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["service"], "ftdata-paid");
    assert!(v["endpoints"].as_array().unwrap().len() >= 4);
}

#[tokio::test]
async fn quote_endpoint_returns_price_without_payment() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let v: Value = resp.json().await.unwrap();
    assert!(v["quote_id"].is_string());
    assert!(v["price_usdc"].is_string());
    assert!(v["payment_required"]["max_amount"].is_string());
}

#[tokio::test]
async fn download_without_payment_header_returns_402() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/download"))
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 402);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "payment_required");
    assert!(v["payment_required"]["quote_id"].is_string());
    assert!(v["payment_required"]["max_amount"].is_string());
}

#[tokio::test]
async fn download_with_valid_payment_completes_job() {
    let (base, m) = spawn_app().await;
    let client = reqwest::Client::new();

    // 1. First request without payment to get the challenge.
    let r1 = client
        .post(format!("{base}/v1/download"))
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 402);
    let challenge: Value = r1.json::<Value>().await.unwrap()["payment_required"].clone();

    // 2. Build a payment proof covering the required amount.
    let proof = PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xAGENT_WALLET".into(),
        amount: challenge["max_amount"].as_str().unwrap().to_string(),
        quote_id: challenge["quote_id"].as_str().unwrap().to_string(),
        signature: "0xMOCK_SIG".into(),
        nonce: "n1".into(),
        valid_until: UnixSecs::now(),
    };
    let proof_json = serde_json::to_string(&proof).unwrap();

    // 3. Retry with X-PAYMENT header.
    let r2 = client
        .post(format!("{base}/v1/download"))
        .header("x-payment", &proof_json)
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 202);
    let v: Value = r2.json().await.unwrap();
    let job_id = v["job_id"].as_str().unwrap().to_string();
    assert_eq!(v["status"], "queued");
    assert_eq!(v["payment_settled"], true);
    assert!(m.facilitator_id() == "mock");

    // 4. Poll the job until completion (Q9: real network download may take longer).
    let mut final_status = String::new();
    for _ in 0..200 { // 200 * 100ms = 20s timeout
        tokio::time::sleep(Duration::from_millis(100)).await;
        let r3 = client
            .get(format!("{base}/v1/jobs/{job_id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(r3.status(), 200);
        let j: Value = r3.json().await.unwrap();
        final_status = j["status"].as_str().unwrap().to_string();
        if final_status == "completed" || final_status == "failed" {
            break;
        }
    }
    assert_eq!(final_status, "completed");
}

#[tokio::test]
async fn download_with_underpayment_returns_402_insufficient() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();

    let r1 = client
        .post(format!("{base}/v1/download"))
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    let challenge: Value = r1.json::<Value>().await.unwrap()["payment_required"].clone();
    let required = challenge["max_amount"].as_str().unwrap().to_string();
    let underpaid = format!("{}.0", required.parse::<f64>().unwrap() - 0.01);

    let proof = PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xCHEAP".into(),
        amount: underpaid,
        quote_id: challenge["quote_id"].as_str().unwrap().to_string(),
        signature: "0xSIG".into(),
        nonce: "n1".into(),
        valid_until: UnixSecs::now(),
    };
    let proof_json = serde_json::to_string(&proof).unwrap();

    let r2 = client
        .post(format!("{base}/v1/download"))
        .header("x-payment", &proof_json)
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 402);
    let v: Value = r2.json().await.unwrap();
    assert_eq!(v["error"], "payment_insufficient");
    assert!(v["required"].is_string());
}

#[tokio::test]
async fn download_with_unknown_quote_id_returns_402_expired() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();

    let proof = PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xAGENT".into(),
        amount: "1.000000".into(),
        quote_id: "qt_FAKE_NOT_FROM_CHALLENGE".into(),
        signature: "0xSIG".into(),
        nonce: "n1".into(),
        valid_until: UnixSecs::now(),
    };
    let proof_json = serde_json::to_string(&proof).unwrap();

    let resp = client
        .post(format!("{base}/v1/download"))
        .header("x-payment", &proof_json)
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 402);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "payment_expired");
}

#[tokio::test]
async fn download_with_bad_payment_header_returns_402_invalid() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/v1/download"))
        .header("x-payment", "not-base64-not-json-garbage")
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 402);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "payment_invalid");
}

#[tokio::test]
async fn unknown_job_id_returns_404() {
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!("{base}/v1/jobs/does-not-exist")).await.unwrap();
    assert_eq!(resp.status(), 404);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "job_not_found");
}

#[tokio::test]
async fn bad_request_body_returns_400() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let bad = json!({
        "exchange": "unknown_exchange",
        "pairs": ["BTC/USDT"],
        "timeframes": ["1m"],
        "timerange": "20230101-"
    });
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&bad)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "bad_request");
}

#[tokio::test]
async fn reconcile_with_no_receipts_returns_zeros() {
    // Q10: reconcile is now auth-required. Use ?wallet= for the customer view.
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!(
        "{base}/v1/reconcile?since=0&until=9999999999&wallet=0xTEST"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["jobs_completed"], 0);
    assert_eq!(v["revenue_total_usdc"], "0.000000");
    assert_eq!(v["net_revenue_usdc"], "0.000000");
}

#[tokio::test]
async fn reconcile_without_auth_returns_402() {
    // Q10: unauthenticated reconcile must fail. Use 402 since auth is
    // payment-shaped for now (x402 / API key as a "proof of who you are").
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!("{base}/v1/reconcile?since=0&until=9999999999"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 402);
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"], "payment_invalid");
}

#[tokio::test]
async fn completed_download_emits_receipt_aggregated_by_reconcile() {
    let (base, m) = spawn_app().await;
    let client = reqwest::Client::new();

    // 1. First request: 402 with challenge.
    let r1 = client
        .post(format!("{base}/v1/download"))
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    let challenge: Value = r1.json::<Value>().await.unwrap()["payment_required"].clone();

    // 2. Build a valid proof with payer "0xRECON_AGENT".
    let payer = "0xRECON_AGENT".to_string();
    let proof = PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: payer.clone(),
        amount: challenge["max_amount"].as_str().unwrap().to_string(),
        quote_id: challenge["quote_id"].as_str().unwrap().to_string(),
        signature: "0xMOCK_SIG".into(),
        nonce: "n1".into(),
        valid_until: UnixSecs::now(),
    };
    let proof_json = serde_json::to_string(&proof).unwrap();

    // 3. Submit with X-PAYMENT.
    let r2 = client
        .post(format!("{base}/v1/download"))
        .header("x-payment", &proof_json)
        .json(&sample_body())
        .send()
        .await
        .unwrap();
    let v: Value = r2.json().await.unwrap();
    let job_id = v["job_id"].as_str().unwrap().to_string();
    let amount = v["amount_paid_usdc"].as_str().unwrap().to_string();
    assert_eq!(v["payment_settled"], true);
    assert_eq!(m.facilitator_id(), "mock");

    // 4. Poll until completed (Q9: real network download may take longer).
    for _ in 0..200 { // 200 * 100ms = 20s timeout
        tokio::time::sleep(Duration::from_millis(100)).await;
        let r3 = client
            .get(format!("{base}/v1/jobs/{job_id}"))
            .send()
            .await
            .unwrap();
        let j: Value = r3.json().await.unwrap();
        if j["status"].as_str() == Some("completed") {
            break;
        }
    }

    // 5. Customer view: pass ?wallet=0xRECON_AGENT to see this customer's receipts.
    let resp = reqwest::get(format!(
        "{base}/v1/reconcile?since=0&until=9999999999&wallet=0xRECON_AGENT"
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 200);
    let report: Value = resp.json().await.unwrap();
    assert_eq!(report["jobs_completed"], 1);
    assert_eq!(report["revenue_total_usdc"].as_str().unwrap(), amount);
    // 1% fee
    let gross: f64 = amount.parse().unwrap();
    let fee = gross * 0.01;
    let net = gross - fee;
    assert!(
        (report["net_revenue_usdc"].as_str().unwrap().parse::<f64>().unwrap() - net).abs()
            < 0.000_001
    );
    // by_exchange aggregates
    assert_eq!(
        report["by_exchange"]["binance"].as_str().unwrap(),
        amount
    );
}

#[tokio::test]
async fn quote_works_for_binance_only_per_q2() {
    // Q2: launch is Binance-only.
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "binance",
            "pairs": ["BTC/USDT"],
            "timeframes": ["1m"],
            "timerange": "20230101-20230108",
            "market": "spot"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn quote_rejects_bybit_and_okx_per_q2() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    for exchange in ["bybit", "okx"] {
        let resp = client
            .post(format!("{base}/v1/quote"))
            .json(&json!({
                "exchange": exchange,
                "pairs": ["BTC/USDT"],
                "timeframes": ["1m"],
                "timerange": "20230101-20230108"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "{} should be rejected", exchange);
        let v: Value = resp.json().await.unwrap();
        assert!(v["message"].as_str().unwrap().contains("Binance-only"));
    }
}

#[tokio::test]
async fn quote_with_lower_resolution_timeframe_is_cheaper() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let q1m = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "binance",
            "pairs": ["BTC/USDT"],
            "timeframes": ["1m"],
            "timerange": "20230101-20230201"
        }))
        .send()
        .await
        .unwrap();
    let q1d = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "binance",
            "pairs": ["BTC/USDT"],
            "timeframes": ["1d"],
            "timerange": "20230101-20230201"
        }))
        .send()
        .await
        .unwrap();
    let p1m: f64 = q1m.json::<Value>().await.unwrap()["price_usdc"].as_str().unwrap().parse().unwrap();
    let p1d: f64 = q1d.json::<Value>().await.unwrap()["price_usdc"].as_str().unwrap().parse().unwrap();
    assert!(p1d < p1m, "1d should be cheaper than 1m, got 1d={p1d} 1m={p1m}");
}

#[tokio::test]
async fn quote_for_futures_market_is_more_expensive_than_spot() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let body = |market: &str| json!({
        "exchange": "binance",
        "pairs": ["BTC/USDT"],
        "timeframes": ["1m"],
        "timerange": "20230101-20230108",
        "market": market
    });
    let spot = client.post(format!("{base}/v1/quote")).json(&body("spot")).send().await.unwrap();
    let fut = client.post(format!("{base}/v1/quote")).json(&body("futures")).send().await.unwrap();
    let ps: f64 = spot.json::<Value>().await.unwrap()["price_usdc"].as_str().unwrap().parse().unwrap();
    let pf: f64 = fut.json::<Value>().await.unwrap()["price_usdc"].as_str().unwrap().parse().unwrap();
    assert!(pf > ps, "futures should be more expensive than spot: spot={ps} futures={pf}");
}

#[tokio::test]
async fn concurrent_downloads_get_distinct_job_ids() {
    let (base, m) = spawn_app().await;
    let client = reqwest::Client::new();

    // Helper: submit one full quote→pay→download cycle.
    async fn one_round(client: &reqwest::Client, base: &str) -> String {
        let r1 = client
            .post(format!("{base}/v1/download"))
            .json(&sample_body())
            .send()
            .await
            .unwrap();
        assert_eq!(r1.status(), 402);
        let challenge: Value = r1.json::<Value>().await.unwrap()["payment_required"].clone();
        let proof = PaymentProof {
            scheme: Scheme::Exact,
            network: Network::Base,
            asset: Asset::Usdc,
            payer: "0xAGENT".into(),
            amount: challenge["max_amount"].as_str().unwrap().to_string(),
            quote_id: challenge["quote_id"].as_str().unwrap().to_string(),
            signature: "0xSIG".into(),
            nonce: "n1".into(),
            valid_until: UnixSecs::now(),
        };
        let r2 = client
            .post(format!("{base}/v1/download"))
            .header("x-payment", serde_json::to_string(&proof).unwrap())
            .json(&sample_body())
            .send()
            .await
            .unwrap();
        assert_eq!(r2.status(), 202);
        r2.json::<Value>().await.unwrap()["job_id"].as_str().unwrap().to_string()
    }

    // Fire 5 concurrent requests.
    let mut handles = Vec::new();
    for _ in 0..5 {
        let c = client.clone();
        let b = base.clone();
        handles.push(tokio::spawn(async move { one_round(&c, &b).await }));
    }
    let mut ids = Vec::new();
    for h in handles {
        ids.push(h.await.unwrap());
    }
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 5, "expected 5 unique job_ids, got {:?}", ids);
    let _ = m; // silence unused
}

#[tokio::test]
async fn unknown_exchange_returns_400() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "kraken",  // not in allowed list
            "pairs": ["BTC/USDT"],
            "timeframes": ["1m"],
            "timerange": "20230101-"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn unsupported_timeframe_returns_400() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "binance",
            "pairs": ["BTC/USDT"],
            "timeframes": ["7m"],  // not in allowed list
            "timerange": "20230101-"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn empty_pairs_returns_400() {
    let (base, _m) = spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/quote"))
        .json(&json!({
            "exchange": "binance",
            "pairs": [],
            "timeframes": ["1m"],
            "timerange": "20230101-"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn info_endpoint_lists_all_five_routes() {
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!("{base}/v1/info")).await.unwrap();
    let v: Value = resp.json().await.unwrap();
    let endpoints: Vec<String> = v["endpoints"].as_array().unwrap().iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    // After settlement addition we now have 5 routes.
    assert!(endpoints.len() >= 5);
    assert!(endpoints.iter().any(|e| e.contains("/v1/quote")));
    assert!(endpoints.iter().any(|e| e.contains("/v1/download")));
    assert!(endpoints.iter().any(|e| e.contains("/v1/jobs")));
    assert!(endpoints.iter().any(|e| e.contains("/v1/info")));
    assert!(endpoints.iter().any(|e| e.contains("/v1/reconcile")));
}
