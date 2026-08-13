//! Integration tests for `CloudflareFacilitator` against a local axum
//! server that mimics Cloudflare MGW's wire format.
//!
//! These tests don't require a real CF account; they run a local server
//! on an ephemeral port and configure the facilitator to point at it.
//! The same wire format is what CF's real MGW endpoint uses.

use std::net::SocketAddr;

use axum::{routing::post, Json, Router};
use ftdata_paid_facilitator::{
    Asset, CloudflareFacilitator, Network, PaymentProof, PaymentVerifier, Scheme, UnixSecs,
    VerificationError,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;

/// Mock CF MGW: a simple axum server that records requests and replies
/// with a configurable response.
struct MockMgw {
    addr: SocketAddr,
    /// What to respond with on the next verify call.
    response: std::sync::Arc<tokio::sync::Mutex<Value>>,
    /// Records of received verify requests.
    received: std::sync::Arc<tokio::sync::Mutex<Vec<Value>>>,
}

async fn start_mock_mgw() -> MockMgw {
    let received = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let response = std::sync::Arc::new(tokio::sync::Mutex::new(json!({
        "verified": true,
        "tx_hash": "0xMOCK_TX_HASH_abc123",
        "payer": "0xRESPONSE_PAYER",
        "amount": "0.087500"
    })));

    let received_for_handler = received.clone();
    let response_for_handler = response.clone();

    let app = Router::new().route(
        "/v1/payment/verify",
        post(move |Json(body): Json<Value>| {
            let received = received_for_handler.clone();
            let response = response_for_handler.clone();
            async move {
                received.lock().await.push(body);
                let resp = response.lock().await.clone();
                Json(resp)
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give the server a moment.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    MockMgw {
        addr,
        response,
        received,
    }
}

fn test_proof(quote_id: &str, amount: &str) -> PaymentProof {
    PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xAGENT_WALLET".into(),
        amount: amount.into(),
        quote_id: quote_id.into(),
        signature: "0xMOCK_SIG".into(),
        nonce: "n1".into(),
        valid_until: UnixSecs::now(),
    }
}

#[tokio::test]
async fn verify_calls_mgw_and_returns_tx_hash() {
    let mgw = start_mock_mgw().await;
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "test-key".to_string(),
        Network::Base,
    );
    let _ = cf
        .prepare_challenge("qt_1".into(), 50_000, 300); // also calls verify path? no, just stores
    let proof = test_proof("qt_1", "0.050000");
    let r = cf.verify(&proof).await.unwrap();
    match r {
        ftdata_paid_facilitator::VerificationResult::Verified {
            payer,
            amount,
            tx_hash,
        } => {
            assert_eq!(payer, "0xRESPONSE_PAYER");
            assert_eq!(amount, "0.087500");
            assert!(tx_hash.starts_with("0x"));
        }
    }
    // Verify the request was made with the expected body shape.
    let received = mgw.received.lock().await;
    assert_eq!(received.len(), 1);
    let body = &received[0];
    assert_eq!(body["proof"]["scheme"], "exact");
    assert_eq!(body["proof"]["network"], "base");
    assert_eq!(body["proof"]["asset"], "usdc");
    assert_eq!(body["proof"]["payer"], "0xAGENT_WALLET");
    assert_eq!(body["proof"]["amount"], "0.050000");
    assert_eq!(body["proof"]["quote_id"], "qt_1");
    assert!(body["required"].is_object());
    assert_eq!(body["required"]["pay_to"], "0xCLOUDFLARE_MGW_SETTLEMENT");
}

#[tokio::test]
async fn verify_maps_insufficient_amount_error() {
    let mgw = start_mock_mgw().await;
    *mgw.response.lock().await = json!({
        "verified": false,
        "code": "insufficient_amount",
        "message": "amount too low",
        "required": "0.100000",
        "got": "0.050000"
    });
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_x", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::InsufficientAmount { got, required } => {
            assert_eq!(got, "0.050000");
            assert_eq!(required, "0.100000");
        }
        other => panic!("expected InsufficientAmount, got {:?}", other),
    }
}

#[tokio::test]
async fn verify_maps_expired_quote_error() {
    let mgw = start_mock_mgw().await;
    *mgw.response.lock().await = json!({
        "verified": false,
        "code": "expired_quote",
        "message": "quote expired"
    });
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_expired", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::UnknownOrExpiredQuote { quote_id } => {
            assert_eq!(quote_id, "qt_expired");
        }
        other => panic!("expected UnknownOrExpiredQuote, got {:?}", other),
    }
}

#[tokio::test]
async fn verify_maps_bad_signature_error() {
    let mgw = start_mock_mgw().await;
    *mgw.response.lock().await = json!({
        "verified": false,
        "code": "bad_signature",
        "message": "signature mismatch"
    });
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_sig", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::BadSignature { reason } => {
            assert!(reason.contains("signature"));
        }
        other => panic!("expected BadSignature, got {:?}", other),
    }
}

#[tokio::test]
async fn verify_maps_unknown_code_to_facilitator_unavailable() {
    let mgw = start_mock_mgw().await;
    *mgw.response.lock().await = json!({
        "verified": false,
        "code": "weird_new_error_type",
        "message": "something we don't recognize"
    });
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_weird", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::FacilitatorUnavailable { reason } => {
            assert!(reason.contains("weird_new_error_type"));
        }
        other => panic!("expected FacilitatorUnavailable, got {:?}", other),
    }
}

#[tokio::test]
async fn verify_maps_malformed_response_to_facilitator_unavailable() {
    let mgw = start_mock_mgw().await;
    *mgw.response.lock().await = json!("not an object");
    let cf = CloudflareFacilitator::new(
        format!("http://{}", mgw.addr),
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_bad", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::FacilitatorUnavailable { .. } => {}
        other => panic!("expected FacilitatorUnavailable, got {:?}", other),
    }
}

#[tokio::test]
async fn verify_maps_network_error_to_facilitator_unavailable() {
    // Point at a port nothing's listening on.
    let cf = CloudflareFacilitator::new(
        "http://127.0.0.1:1".to_string(), // port 1 is reserved/closed
        "k".to_string(),
        Network::Base,
    );
    let proof = test_proof("qt_net", "0.050000");
    let err = cf.verify(&proof).await.unwrap_err();
    match err {
        VerificationError::FacilitatorUnavailable { reason } => {
            assert!(reason.contains("network error"));
        }
        other => panic!("expected FacilitatorUnavailable, got {:?}", other),
    }
}
