//! Integration tests for `MockFacilitator`.
//!
//! Exercises the 402 retry flow end-to-end at the trait level (without HTTP).

use ftdata_paid_facilitator::{
    Asset, MockFacilitator, Network, PaymentProof, PaymentVerifier, Scheme, UnixSecs,
    VerificationError,
};

fn proof(quote_id: &str, amount: &str) -> PaymentProof {
    PaymentProof {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        payer: "0xAGENT".to_string(),
        amount: amount.to_string(),
        quote_id: quote_id.to_string(),
        signature: "0xSIG".to_string(),
        nonce: "n1".to_string(),
        valid_until: UnixSecs::now(),
    }
}

#[tokio::test]
async fn verifies_when_amount_covers_required() {
    let m = MockFacilitator::new();
    let _ = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_1".into(),
        50_000, // 0.050000 USDC in micro-units
        300,
    );
    let p = proof("qt_1", "0.050000");
    let r = m.verify(&p).await.unwrap();
    match r {
        ftdata_paid_facilitator::VerificationResult::Verified { payer, amount, .. } => {
            assert_eq!(payer, "0xAGENT");
            assert_eq!(amount, "0.050000");
        }
    }
}

#[tokio::test]
async fn overpayment_also_verifies() {
    let m = MockFacilitator::new();
    let _ = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_1".into(),
        50_000,
        300,
    );
    let p = proof("qt_1", "0.100000");
    assert!(m.verify(&p).await.is_ok());
}

#[tokio::test]
async fn underpayment_is_rejected() {
    let m = MockFacilitator::new();
    let _ = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_1".into(),
        50_000,
        300,
    );
    let p = proof("qt_1", "0.040000");
    let err = m.verify(&p).await.unwrap_err();
    assert!(matches!(err, VerificationError::InsufficientAmount { .. }));
}

#[tokio::test]
async fn unknown_quote_is_rejected() {
    let m = MockFacilitator::new();
    let _ = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_1".into(),
        50_000,
        300,
    );
    let p = proof("qt_WRONG", "0.050000");
    let err = m.verify(&p).await.unwrap_err();
    assert!(matches!(err, VerificationError::UnknownOrExpiredQuote { .. }));
}

#[tokio::test]
async fn expired_quote_is_rejected() {
    let m = MockFacilitator::new();
    let mut challenge = m.prepare_challenge_pub("qt_1", "0.050000");
    let now = UnixSecs::now().0;
    challenge.expires_at = UnixSecs(now.saturating_sub(3600));
    m.replace_challenge_for_test("qt_1", challenge);
    let p = proof("qt_1", "0.050000");
    let err = m.verify(&p).await.unwrap_err();
    assert!(matches!(err, VerificationError::UnknownOrExpiredQuote { .. }));
}

#[tokio::test]
async fn forced_error_short_circuits_verify() {
    let m = MockFacilitator::new();
    let _ = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_1".into(),
        50_000,
        300,
    );
    m.force_error(VerificationError::BadSignature {
        reason: "test".to_string(),
    });
    let p = proof("qt_1", "0.050000");
    let err = m.verify(&p).await.unwrap_err();
    assert!(matches!(err, VerificationError::BadSignature { .. }));
}

#[tokio::test]
async fn facilitator_id_is_mock() {
    let m = MockFacilitator::new();
    assert_eq!(m.facilitator_id(), "mock");
}

#[tokio::test]
async fn payment_required_serializes_to_canonical_shape() {
    let m = MockFacilitator::new();
    let challenge = <MockFacilitator as PaymentVerifier>::prepare_challenge(
        &m,
        "qt_x".into(),
        1_000_000,
        300,
    );
    let json = serde_json::to_value(&challenge).unwrap();
    assert_eq!(json["scheme"], "exact");
    assert_eq!(json["network"], "base");
    assert_eq!(json["asset"], "usdc");
    assert_eq!(json["quote_id"], "qt_x");
    assert_eq!(json["max_amount"], "1.000000");
}
