//! Error types for the Dfns SDK.

use serde::Deserialize;

/// A structured error returned by the Dfns API.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "code", default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// All error variants surfaced by the SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API returned a non-2xx status with a decodable error body.
    #[error("dfns api error ({status}): {}", .body.message)]
    Api { status: u16, body: ApiError },

    /// The API returned a non-2xx status with an undecodable body.
    #[error("dfns api error ({status}): {raw}")]
    ApiRaw { status: u16, raw: String },

    /// Invalid client configuration (base URL or auth token).
    #[error("{0}")]
    Config(String),

    /// A user-action-signed request was issued without a configured signer.
    #[error("this operation requires a signer but none was configured")]
    SignerRequired,

    /// The signer failed to produce an assertion.
    #[error("signer error: {0}")]
    Signer(String),

    /// Transport-level failure (connection, timeout, ...).
    #[error("http transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// (De)serialization failure.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
