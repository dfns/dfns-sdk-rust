# Dfns Rust SDK

[![Crates.io](https://img.shields.io/crates/v/dfns-sdk-rust.svg)](https://crates.io/crates/dfns-sdk-rust)
[![Downloads](https://img.shields.io/crates/d/dfns-sdk-rust.svg)](https://crates.io/crates/dfns-sdk-rust)
[![docs.rs](https://img.shields.io/docsrs/dfns-sdk-rust)](https://docs.rs/dfns-sdk-rust)
[![CI](https://github.com/dfns/dfns-sdk-rust/actions/workflows/ci.yaml/badge.svg)](https://github.com/dfns/dfns-sdk-rust/actions/workflows/ci.yaml)
[![MSRV](https://img.shields.io/badge/MSRV-1.75-blue)](https://github.com/dfns/dfns-sdk-rust)
[![License: MIT](https://img.shields.io/crates/l/dfns-sdk-rust.svg)](https://github.com/dfns/dfns-sdk-rust/blob/main/LICENSE)

Welcome, builders. This repo holds the Dfns Rust SDK. Useful links:

- [Dfns Website](https://www.dfns.co)
- [Dfns API Docs](https://docs.dfns.co)

## Installation

```bash
cargo add dfns-sdk-rust
```

## Quick Start

```rust
use dfns_sdk_rust::{DfnsClient, Options};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create the client (read-only operations)
    let client = DfnsClient::new(Options {
        base_url: String::new(), // defaults to https://api.dfns.io
        auth_token: "your-auth-token".to_string(),
        signer: None,
        http: None,
    })?;

    // List wallets
    let wallets = client.wallets.list_wallets(None).await?;
    println!("Found {} wallets", wallets.items.len());

    Ok(())
}
```

## User Action Signing

Some operations (like creating wallets or signing transactions) require user action signing.
The transport handles the challenge dance (`/auth/action/init` -> sign -> `/auth/action`)
automatically; you provide the signing step by implementing the `UserActionSigner` trait
with your credential's key material:

```rust
use std::sync::Arc;

use async_trait::async_trait;
use dfns_sdk_rust::error::Error;
use dfns_sdk_rust::signer::{CredentialAssertion, UserActionChallenge, UserActionSigner};
use dfns_sdk_rust::wallets::types::CreateWalletRequest;
use dfns_sdk_rust::{DfnsClient, Options};

struct MyKeySigner {
    // your credential ID and private key material
}

#[async_trait]
impl UserActionSigner for MyKeySigner {
    async fn sign(&self, challenge: &UserActionChallenge) -> Result<CredentialAssertion, Error> {
        // Sign challenge.challenge with your credential's private key
        // (e.g. Ed25519 or ECDSA P-256) and return the assertion.
        todo!()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = DfnsClient::new(Options {
        base_url: String::new(),
        auth_token: "your-auth-token".to_string(),
        signer: Some(Arc::new(MyKeySigner { /* ... */ })),
        http: None,
    })?;

    // Operations requiring signatures will automatically sign
    let wallet = client
        .wallets
        .create_wallet(CreateWalletRequest {
            network: "EthereumSepolia".to_string(),
            name: None,
            signing_key: None,
            delegate_to: None,
            delay_delegation: None,
            external_id: None,
            tags: None,
        })
        .await?;
    println!("Created wallet: {}", wallet.id);

    Ok(())
}
```

## Available Domains

The client provides access to the following API domains:

- `client.address_watches` - Address watch operations (6 endpoints)
- `client.agreements` - Agreement management (2 endpoints)
- `client.allocations` - Allocation management (6 endpoints)
- `client.auth` - Authentication and user management (56 endpoints)
- `client.exchanges` - Exchange integrations (9 endpoints)
- `client.fee_sponsors` - Fee sponsor management (7 endpoints)
- `client.keys` - Key management (12 endpoints)
- `client.networks` - Network information (7 endpoints)
- `client.payins` - Payin operations (6 endpoints)
- `client.payouts` - Payout operations (5 endpoints)
- `client.permissions` - Permission management (8 endpoints)
- `client.policies` - Policy management (8 endpoints)
- `client.signers` - Signer management (17 endpoints)
- `client.staking` - Staking operations (6 endpoints)
- `client.swaps` - Token swap operations (5 endpoints)
- `client.vaults` - Vault operations (11 endpoints)
- `client.wallets` - Wallet operations (29 endpoints)
- `client.webhooks` - Webhook subscriptions (8 endpoints)

Each domain provides typed methods for all available API endpoints.

## Error Handling

All methods return `Result<T, dfns_sdk_rust::Error>`:

```rust
use dfns_sdk_rust::Error;

match client.wallets.get_wallet("invalid-wallet-id".to_string()).await {
    Ok(wallet) => println!("Wallet: {}", wallet.id),
    Err(Error::Api { status, body }) => {
        eprintln!("API error (status {}): {}", status, body.message);
    }
    Err(err) => eprintln!("Error: {}", err),
}
```

The `Error` enum covers API errors (decoded and raw), configuration errors, missing or
failing signers, transport failures, and (de)serialization failures.

## License

MIT License - See LICENSE file for details.
