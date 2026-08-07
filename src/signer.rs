//! User-action signing.
//!
//! Write operations on the Dfns API require a *user action signature*: the client
//! requests a challenge, signs it with the caller's credential, and replays the
//! assertion on the real request. The signing keys never leave the caller.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// The challenge returned by `POST /auth/action/init`, to be signed by the credential.
#[derive(Debug, Clone, Deserialize)]
pub struct UserActionChallenge {
    #[serde(rename = "challenge")]
    pub challenge: String,
    #[serde(rename = "challengeIdentifier")]
    pub challenge_identifier: String,
}

/// The credential-specific payload of a signed assertion.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialAssertionData {
    #[serde(rename = "credId")]
    pub cred_id: String,
    #[serde(rename = "clientData")]
    pub client_data: String,
    #[serde(rename = "signature")]
    pub signature: String,
}

/// A signed challenge assertion, sent as the `firstFactor` of `POST /auth/action`.
#[derive(Debug, Clone, Serialize)]
pub struct CredentialAssertion {
    #[serde(rename = "kind")]
    pub kind: String,
    #[serde(rename = "credentialAssertion")]
    pub credential_assertion: CredentialAssertionData,
}

/// Implemented by credential backends (WebAuthn, raw key, KMS, ...).
///
/// The signer only signs the challenge; the transport owns the init/complete dance and
/// turns the assertion into the `X-DFNS-USERACTION` token. This trait is intentionally
/// minimal and hand-maintained: real implementations handle the credential-specific crypto
/// and live outside the generated code.
#[async_trait]
pub trait UserActionSigner: Send + Sync {
    async fn sign(&self, challenge: &UserActionChallenge) -> Result<CredentialAssertion, Error>;
}
