//! Repo-owned integration test that the `#[deprecated]` forwarding alias emitted by
//! the operationId rename issues the identical HTTP request as its canonical method.
//!
//! Endpoint under test: exchanges "create deposit" — canonical `create_deposit` with
//! deprecated alias `create_exchange_deposit`. A "new"-decision row: a POST with a typed
//! body, so we assert method + path + body forwarding, not just that the alias compiles.
//!
//! Follows `tests/client.rs` (wiremock). `create_deposit` requires a user action, so the
//! client is given a MockSigner and the /auth/action(/init) endpoints are stubbed exactly
//! as in that file. The deposits mock is set to `.expect(2)`: both the canonical and the
//! deprecated call must match the same method + path + body matcher, or the mock server
//! fails verification on drop — which is what proves the alias forwards identically.
#![allow(deprecated)] // calling the deprecated alias is the whole point of this test

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use dfns_sdk_rust::error::Error;
use dfns_sdk_rust::exchanges::ExchangesClient;
use dfns_sdk_rust::signer::{
    CredentialAssertion, CredentialAssertionData, UserActionChallenge, UserActionSigner,
};
use dfns_sdk_rust::{Client, Options};

/// A signer that returns a fixed assertion for the stubbed challenge.
struct MockSigner;

#[async_trait]
impl UserActionSigner for MockSigner {
    async fn sign(&self, challenge: &UserActionChallenge) -> Result<CredentialAssertion, Error> {
        assert_eq!(challenge.challenge_identifier, "challenge-123");
        Ok(CredentialAssertion {
            kind: "Key".to_string(),
            credential_assertion: CredentialAssertionData {
                cred_id: "cred-1".to_string(),
                client_data: "Y2xpZW50LWRhdGE".to_string(),
                signature: "c2lnbmF0dXJl".to_string(),
            },
        })
    }
}

fn exchanges_client(server: &MockServer) -> ExchangesClient {
    let client = Client::new(Options {
        base_url: server.uri(),
        auth_token: "test-token".to_string(),
        signer: Some(Arc::new(MockSigner)),
        http: None,
    })
    .unwrap();
    ExchangesClient::new(client)
}

async fn mount_user_action_stubs(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/auth/action/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "challenge": "dGVzdC1jaGFsbGVuZ2U",
            "challengeIdentifier": "challenge-123",
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/action"))
        .and(body_partial_json(
            json!({ "challengeIdentifier": "challenge-123" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "userAction": "user-action-token-xyz",
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn deprecated_alias_forwards_to_same_request_as_canonical() {
    let server = MockServer::start().await;
    mount_user_action_stubs(&server).await;

    let deposit_path = "/exchanges/ex-1/accounts/acc-1/deposits";
    let deposit_body = json!({ "kind": "Native", "asset": "ETH", "amount": "1.5" });

    // The single deposits stub must be hit exactly twice — once by the canonical method
    // and once by the deprecated alias — with the same POST, path, signed header and body.
    // If the alias diverged on any of these, this mock would not match and verification
    // (on server drop) would fail.
    Mock::given(method("POST"))
        .and(path(deposit_path))
        .and(header("x-dfns-useraction", "user-action-token-xyz"))
        .and(body_partial_json(deposit_body.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "dp-1",
            "exchangeId": "ex-1",
            "accountId": "acc-1",
            "kind": "Native",
            "walletId": "wa-1",
            "requester": { "userId": "us-1" },
            "requestBody": deposit_body,
            "dateCreated": "2026-01-01T00:00:00Z",
        })))
        .expect(2)
        .mount(&server)
        .await;

    let client = exchanges_client(&server);

    let via_canonical = client
        .create_deposit(
            "ex-1".to_string(),
            "acc-1".to_string(),
            deposit_body.clone(),
        )
        .await
        .expect("canonical create_deposit should succeed");

    let via_alias = client
        .create_exchange_deposit(
            "ex-1".to_string(),
            "acc-1".to_string(),
            deposit_body.clone(),
        )
        .await
        .expect("deprecated create_exchange_deposit should succeed");

    assert_eq!(via_canonical.id, "dp-1");
    assert_eq!(via_alias.id, "dp-1");
    // Both resolve to the same canonical response type and identical decoded content.
    assert_eq!(via_canonical.exchange_id, via_alias.exchange_id);
    assert_eq!(via_canonical.account_id, via_alias.account_id);

    // Expectation of exactly 2 matching requests is checked here (and again on drop).
    server.verify().await;
}
