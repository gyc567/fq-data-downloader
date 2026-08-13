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
        "timerange": "20230101-20230201",
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

    // 4. Poll the job until completion.
    let mut final_status = String::new();
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(50)).await;
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
    let (base, _m) = spawn_app().await;
    let resp = reqwest::get(format!(
        "{base}/v1/reconcile?since=0&until=9999999999"
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

    // 2. Build a valid proof.
    let proof = PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xAGENT".into(),
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

    // 4. Poll until completed.
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(50)).await;
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

    // 5. Reconcile should now report one completed job with revenue.
    let resp = reqwest::get(format!(
        "{base}/v1/reconcile?since=0&until=9999999999"
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
