//! Integration tests for the transport core, against a mock HTTP server.
//! Hand-written and maintained in this repo
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Method;
use serde_json::json;
use sha2::{Digest, Sha256};
use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use dfns_sdk_rust::client::{Client, MultipartFile, Options};
use dfns_sdk_rust::error::Error;
use dfns_sdk_rust::signer::{
    CredentialAssertion, CredentialAssertionData, UserActionChallenge, UserActionSigner,
};

/// A signer that returns a fixed assertion, asserting the challenge it receives.
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

/// A signer that always fails, standing in for an unavailable signing device.
struct ErrSigner;

#[async_trait]
impl UserActionSigner for ErrSigner {
    async fn sign(&self, _challenge: &UserActionChallenge) -> Result<CredentialAssertion, Error> {
        Err(Error::Signer("signing device unavailable".to_string()))
    }
}

fn client(server: &MockServer, signer: Option<Arc<dyn UserActionSigner>>) -> Client {
    Client::new(Options {
        base_url: server.uri(),
        auth_token: "test-token".to_string(),
        signer,
        http: None,
    })
    .unwrap()
}

#[test]
fn new_defaults_empty_base_url_to_https() {
    let c = Client::new(Options {
        base_url: String::new(),
        auth_token: "t".to_string(),
        signer: None,
        http: None,
    });
    assert!(c.is_ok());
}

#[test]
fn new_rejects_external_http() {
    let res = Client::new(Options {
        base_url: "http://api.dfns.io".to_string(),
        auth_token: "t".to_string(),
        signer: None,
        http: None,
    });
    assert!(matches!(res, Err(Error::Config(_))));
}

#[test]
fn new_allows_loopback_http() {
    let c = Client::new(Options {
        base_url: "http://127.0.0.1:8080".to_string(),
        auth_token: "t".to_string(),
        signer: None,
        http: None,
    });
    assert!(c.is_ok());
}

#[test]
fn new_requires_auth_token() {
    let res = Client::new(Options {
        base_url: "https://api.dfns.io".to_string(),
        auth_token: String::new(),
        signer: None,
        http: None,
    });
    assert!(matches!(res, Err(Error::Config(_))));
}

/// Mount the two dance endpoints returning a fixed challenge and token.
async fn mount_dance(server: &MockServer) {
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
async fn requires_signature_no_signer() {
    let server = MockServer::start().await;
    let c = client(&server, None);

    let err = c
        .request_no_content(Method::POST, "/test", None, true)
        .await
        .unwrap_err();

    assert!(matches!(err, Error::SignerRequired));
}

#[tokio::test]
async fn requires_signature_full_flow() {
    let server = MockServer::start().await;

    // The init challenge must carry the canonical (query-free) path and the exact body.
    Mock::given(method("POST"))
        .and(path("/auth/action/init"))
        .and(body_partial_json(json!({
            "userActionHttpMethod": "POST",
            "userActionHttpPath": "/test/endpoint",
            "userActionServerKind": "Api",
            "userActionPayload": "{\"key\":\"value\"}",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "challenge": "dGVzdC1jaGFsbGVuZ2U",
            "challengeIdentifier": "challenge-123",
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/auth/action"))
        .and(body_partial_json(
            json!({ "challengeIdentifier": "challenge-123" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "userAction": "user-action-token-xyz",
        })))
        .mount(&server)
        .await;

    // The real request only matches when the signed token is attached as the header.
    Mock::given(method("POST"))
        .and(path("/test/endpoint"))
        .and(header("x-dfns-useraction", "user-action-token-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": "ok" })))
        .mount(&server)
        .await;

    let body = json!({ "key": "value" });
    let out: serde_json::Value = client(&server, Some(Arc::new(MockSigner)))
        .request(Method::POST, "/test/endpoint?page=1", Some(&body), true)
        .await
        .unwrap();

    assert_eq!(out["result"], "ok");
}

#[tokio::test]
async fn requires_signature_signer_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/action/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "challenge": "dGVzdA",
            "challengeIdentifier": "challenge-123",
        })))
        .mount(&server)
        .await;

    let err = client(&server, Some(Arc::new(ErrSigner)))
        .request_no_content(Method::POST, "/test", None, true)
        .await
        .unwrap_err();

    assert!(matches!(err, Error::Signer(_)));
}

#[tokio::test]
async fn requires_signature_challenge_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/action/init"))
        .respond_with(ResponseTemplate::new(500).set_body_string("server error"))
        .mount(&server)
        .await;

    let err = client(&server, Some(Arc::new(MockSigner)))
        .request_no_content(Method::POST, "/test", None, true)
        .await
        .unwrap_err();

    assert!(matches!(err, Error::Api { .. } | Error::ApiRaw { .. }));
}

#[tokio::test]
async fn multipart_success() {
    let server = MockServer::start().await;
    let file_bytes = b"hello-file-contents".to_vec();
    let checksum = hex::encode(Sha256::digest(&file_bytes));

    // The multipart body must carry the JSON field, the injected fileChecksum, and the file.
    Mock::given(method("POST"))
        .and(path("/upload"))
        .and(body_string_contains("name=\"data\""))
        .and(body_string_contains("\"network\":\"Eth\""))
        .and(body_string_contains(format!(
            "\"fileChecksum\":\"{}\"",
            checksum
        )))
        .and(body_string_contains("filename=\"doc.pdf\""))
        .and(body_string_contains("hello-file-contents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&server)
        .await;

    let body = json!({ "network": "Eth" });
    let file = MultipartFile {
        file_name: "doc.pdf".to_string(),
        bytes: file_bytes,
    };
    let out: serde_json::Value = client(&server, None)
        .request_multipart(Method::POST, "/upload", Some(&body), file, false)
        .await
        .unwrap();

    assert_eq!(out["ok"], true);
}

#[tokio::test]
async fn multipart_default_file_name() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload"))
        .and(body_string_contains("filename=\"upload.bin\""))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let file = MultipartFile {
        file_name: String::new(),
        bytes: b"x".to_vec(),
    };
    client(&server, None)
        .request_multipart_no_content(Method::POST, "/upload", None, file, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn multipart_file_name_sanitized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/upload"))
        .and(body_string_contains("filename=\"ab.bin\""))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let file = MultipartFile {
        file_name: "a\r\nb.bin".to_string(),
        bytes: b"x".to_vec(),
    };
    client(&server, None)
        .request_multipart_no_content(Method::POST, "/upload", None, file, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn multipart_requires_signature_full_flow() {
    let server = MockServer::start().await;
    mount_dance(&server).await;

    Mock::given(method("POST"))
        .and(path("/upload"))
        .and(header("x-dfns-useraction", "user-action-token-xyz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "result": "ok" })))
        .mount(&server)
        .await;

    let body = json!({ "k": "v" });
    let file = MultipartFile {
        file_name: "f.bin".to_string(),
        bytes: b"file-bytes".to_vec(),
    };
    let out: serde_json::Value = client(&server, Some(Arc::new(MockSigner)))
        .request_multipart(Method::POST, "/upload", Some(&body), file, true)
        .await
        .unwrap();

    assert_eq!(out["result"], "ok");
}
