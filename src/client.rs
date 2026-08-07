//! Core HTTP transport shared by every generated domain client.
//!
//! Hand-written: owns auth headers, base URL, user-action signing hand-off, and the
//! request/response plumbing the generated methods call into.

use std::sync::Arc;

use reqwest::header::CONTENT_TYPE;
use reqwest::multipart::{Form, Part};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{ApiError, Error};
use crate::signer::{CredentialAssertion, UserActionChallenge, UserActionSigner};

/// A file part for multipart uploads.
#[derive(Debug, Clone)]
pub struct MultipartFile {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

/// Configuration for the transport client.
pub struct Options {
    pub base_url: String,
    pub auth_token: String,
    pub signer: Option<Arc<dyn UserActionSigner>>,
    pub http: Option<reqwest::Client>,
}

/// Request body for `POST /auth/action/init`.
#[derive(Serialize)]
struct InitRequest {
    #[serde(rename = "userActionPayload")]
    user_action_payload: String,
    #[serde(rename = "userActionHttpMethod")]
    user_action_http_method: String,
    #[serde(rename = "userActionHttpPath")]
    user_action_http_path: String,
    #[serde(rename = "userActionServerKind")]
    user_action_server_kind: String,
}

/// Request body for `POST /auth/action`.
#[derive(Serialize)]
struct CompleteRequest<'a> {
    #[serde(rename = "challengeIdentifier")]
    challenge_identifier: String,
    #[serde(rename = "firstFactor")]
    first_factor: &'a CredentialAssertion,
}

#[derive(serde::Deserialize)]
struct CompleteResponse {
    #[serde(rename = "userAction")]
    user_action: String,
}

/// The shared transport. Cheap to clone (all state is Arc-backed).
#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

struct ClientInner {
    base_url: String,
    auth_token: String,
    signer: Option<Arc<dyn UserActionSigner>>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(opts: Options) -> Result<Self, Error> {
        let mut base_url = opts.base_url;
        if base_url.is_empty() {
            base_url = "https://api.dfns.io".to_string();
        }

        // https is required so the bearer token never crosses the wire in cleartext.
        // Plain http is allowed only for loopback hosts (local development and tests),
        // where nothing leaves the machine.
        let parsed = reqwest::Url::parse(&base_url)
            .map_err(|e| Error::Config(format!("invalid BaseURL: {e}")))?;
        if parsed.scheme() != "https" && !is_loopback_host(&parsed) {
            return Err(Error::Config(
                "BaseURL must use https scheme (http allowed only for loopback)".to_string(),
            ));
        }

        if opts.auth_token.is_empty() {
            return Err(Error::Config("AuthToken is required".to_string()));
        }

        let http = match opts.http {
            Some(client) => client,
            None => reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        };

        Ok(Client {
            inner: Arc::new(ClientInner {
                base_url: base_url.trim_end_matches('/').to_string(),
                auth_token: opts.auth_token,
                signer: opts.signer,
                http,
            }),
        })
    }

    /// Perform a request and decode a typed response.
    pub async fn request<R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        requires_user_action: bool,
    ) -> Result<R, Error> {
        let bytes = self.send(method, path, body, requires_user_action).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Perform a request that returns no meaningful body.
    pub async fn request_no_content(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        requires_user_action: bool,
    ) -> Result<(), Error> {
        self.send(method, path, body, requires_user_action).await?;
        Ok(())
    }

    /// Perform a multipart/form-data upload and decode a typed response.
    pub async fn request_multipart<R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        file: MultipartFile,
        requires_user_action: bool,
    ) -> Result<R, Error> {
        let bytes = self
            .send_multipart(method, path, body, file, requires_user_action)
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Perform a multipart/form-data upload that returns no meaningful body.
    pub async fn request_multipart_no_content(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        file: MultipartFile,
        requires_user_action: bool,
    ) -> Result<(), Error> {
        self.send_multipart(method, path, body, file, requires_user_action)
            .await?;
        Ok(())
    }

    /// Returns the challenge to sign for (method, path, body). The request must later
    /// be issued with the same (method, path, body).
    pub async fn create_user_action_challenge(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<UserActionChallenge, Error> {
        let payload: Vec<u8> = match body {
            Some(b) => serde_json::to_vec(b)?,
            None => Vec::new(),
        };
        self.init_user_action_challenge(&method, path, &payload)
            .await
    }

    /// Submits an externally-signed assertion and returns the user action token.
    pub async fn complete_user_action_signing(
        &self,
        challenge_identifier: String,
        assertion: &CredentialAssertion,
    ) -> Result<String, Error> {
        let complete = CompleteRequest {
            challenge_identifier,
            first_factor: assertion,
        };
        let resp: CompleteResponse = self.post_json("/auth/action", &complete).await?;
        Ok(resp.user_action)
    }

    /// Performs a request with an already obtained user action token.
    pub async fn request_with_user_action<R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        user_action_token: &str,
    ) -> Result<R, Error> {
        let payload: Vec<u8> = match body {
            Some(b) => serde_json::to_vec(b)?,
            None => Vec::new(),
        };
        let bytes = self
            .dispatch(
                method,
                path,
                body.is_some().then_some(payload),
                Some(user_action_token),
            )
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Same but without decoding a response body.
    pub async fn request_no_content_with_user_action(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        user_action_token: &str,
    ) -> Result<(), Error> {
        let payload: Vec<u8> = match body {
            Some(b) => serde_json::to_vec(b)?,
            None => Vec::new(),
        };
        self.dispatch(
            method,
            path,
            body.is_some().then_some(payload),
            Some(user_action_token),
        )
        .await?;
        Ok(())
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        requires_user_action: bool,
    ) -> Result<Vec<u8>, Error> {
        // Serialize the body once and send exactly those bytes, so the signed
        // userActionPayload byte-matches what the server receives.
        let payload: Vec<u8> = match body {
            Some(b) => serde_json::to_vec(b)?,
            None => Vec::new(),
        };

        let token = if requires_user_action {
            Some(self.user_action_token(&method, path, &payload).await?)
        } else {
            None
        };

        self.dispatch(
            method,
            path,
            body.is_some().then_some(payload),
            token.as_deref(),
        )
        .await
    }

    async fn dispatch(
        &self,
        method: Method,
        path: &str,
        payload: Option<Vec<u8>>,
        user_action_token: Option<&str>,
    ) -> Result<Vec<u8>, Error> {
        let url = format!("{}{}", self.inner.base_url, path);
        let mut req = self
            .inner
            .http
            .request(method, &url)
            .bearer_auth(&self.inner.auth_token);
        if let Some(payload) = payload {
            req = req.header(CONTENT_TYPE, "application/json").body(payload);
        }
        if let Some(token) = user_action_token {
            req = req.header("X-DFNS-USERACTION", token);
        }
        Self::handle_response(req.send().await?).await
    }

    async fn send_multipart(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        file: MultipartFile,
        requires_user_action: bool,
    ) -> Result<Vec<u8>, Error> {
        // The "data" part is the JSON body plus the file checksum the API expects.
        let mut data = serde_json::Map::new();
        if let Some(serde_json::Value::Object(m)) = body {
            data = m.clone();
        }
        let checksum = hex::encode(Sha256::digest(&file.bytes));
        data.insert(
            "fileChecksum".to_string(),
            serde_json::Value::String(checksum),
        );
        let data_bytes = serde_json::to_vec(&serde_json::Value::Object(data))?;

        // Strip CR/LF from the caller-supplied name to prevent header injection.
        let mut file_name = file.file_name.replace(['\r', '\n'], "");
        if file_name.is_empty() {
            file_name = "upload.bin".to_string();
        }

        let form = Form::new()
            .text("data", String::from_utf8_lossy(&data_bytes).into_owned())
            .part("file", Part::bytes(file.bytes).file_name(file_name));

        let url = format!("{}{}", self.inner.base_url, path);
        let mut req = self
            .inner
            .http
            .request(method.clone(), &url)
            .bearer_auth(&self.inner.auth_token)
            .multipart(form);

        // User action signing covers the "data" payload, matching the JSON body path.
        if requires_user_action {
            let token = self.user_action_token(&method, path, &data_bytes).await?;
            req = req.header("X-DFNS-USERACTION", token);
        }

        Self::handle_response(req.send().await?).await
    }

    /// Run the three-step user-action dance and return the resulting token.
    async fn user_action_token(
        &self,
        method: &Method,
        path: &str,
        payload: &[u8],
    ) -> Result<String, Error> {
        let signer = self.inner.signer.as_ref().ok_or(Error::SignerRequired)?;
        let challenge = self
            .init_user_action_challenge(method, path, payload)
            .await?;
        let assertion = signer.sign(&challenge).await?;
        self.complete_user_action_signing(challenge.challenge_identifier, &assertion)
            .await
    }

    async fn init_user_action_challenge(
        &self,
        method: &Method,
        path: &str,
        payload: &[u8],
    ) -> Result<UserActionChallenge, Error> {
        let init = InitRequest {
            user_action_payload: String::from_utf8_lossy(payload).into_owned(),
            user_action_http_method: method.as_str().to_string(),
            user_action_http_path: canonical_request_path(path),
            user_action_server_kind: "Api".to_string(),
        };
        self.post_json("/auth/action/init", &init).await
    }

    /// POST a JSON body and decode a typed response, without user-action signing.
    async fn post_json<B, R>(&self, path: &str, body: &B) -> Result<R, Error>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let value = serde_json::to_value(body)?;
        // Box::pin breaks the send -> user_action_token -> post_json -> send async recursion cycle.
        let bytes = Box::pin(self.send(Method::POST, path, Some(&value), false)).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn handle_response(resp: reqwest::Response) -> Result<Vec<u8>, Error> {
        let status = resp.status();
        let bytes = resp.bytes().await?.to_vec();

        if !status.is_success() {
            return Err(match serde_json::from_slice::<ApiError>(&bytes) {
                Ok(body) => Error::Api {
                    status: status.as_u16(),
                    body,
                },
                Err(_) => Error::ApiRaw {
                    status: status.as_u16(),
                    raw: String::from_utf8_lossy(&bytes).into_owned(),
                },
            });
        }

        Ok(bytes)
    }
}

/// Whether the URL points at a loopback host, for which plain http is tolerated.
fn is_loopback_host(url: &reqwest::Url) -> bool {
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
        None => false,
    }
}

/// Removes the transport query from the path bound into a user-action challenge.
/// Query parameters are transported on the real request but are not part of the
/// canonical Dfns userActionHttpPath.
fn canonical_request_path(path: &str) -> String {
    match path.split_once('?') {
        Some((head, _)) => head.to_string(),
        None => path.to_string(),
    }
}
