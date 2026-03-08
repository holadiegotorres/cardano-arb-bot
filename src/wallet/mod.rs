//! Wallet Manager
//!
//! Handles key management, UTXO tracking, and transaction signing.
//! Supports Ed25519 signing keys in standard Cardano formats.

use anyhow::{Context, Result};
use tracing::info;

use crate::config::WalletConfig;

pub struct WalletManager {
    /// The signing key bytes (Ed25519 private key)
    signing_key: Vec<u8>,
    /// The verification (public) key bytes
    verification_key: Vec<u8>,
    /// Bech32-encoded address
    address: String,
    /// Configuration
    config: WalletConfig,
}

impl WalletManager {
    pub fn new(config: &WalletConfig) -> Result<Self> {
        // Load signing key from file
        let key_content = std::fs::read_to_string(&config.signing_key_path)
            .with_context(|| {
                format!("Failed to read signing key from: {}", config.signing_key_path)
            })?;

        let key_content = key_content.trim();

        // Parse key — supports both CBOR hex and text envelope formats
        let (signing_key, verification_key) = if key_content.starts_with('{') {
            // Text envelope format (cardano-cli style)
            parse_text_envelope(key_content)?
        } else {
            // Raw hex
            parse_hex_key(key_content)?
        };

        // Derive address from verification key
        let address = derive_address(&verification_key)?;

        info!(
            "Wallet initialized | address: {} | min_reserve: {} ADA | max_trade: {} ADA",
            address, config.min_ada_reserve, config.max_trade_amount_ada
        );

        Ok(Self {
            signing_key,
            verification_key,
            address,
            config: config.clone(),
        })
    }

    pub fn address_bech32(&self) -> &str {
        &self.address
    }

    /// Sign a transaction (CBOR bytes) and return the signed transaction bytes
    pub fn sign_transaction(&self, tx_body: &[u8]) -> Result<Vec<u8>> {
        // In production, this would:
        // 1. Hash the transaction body with Blake2b-256
        // 2. Sign the hash with Ed25519
        // 3. Construct the full transaction with witness set
        //
        // Using pallas-crypto:
        // let hash = pallas_crypto::hash::Hasher::<256>::hash(tx_body);
        // let signature = ed25519_dalek::SigningKey::from_bytes(&self.signing_key)
        //     .sign(&hash);

        // Placeholder
        let mut signed = tx_body.to_vec();
        signed.extend_from_slice(&[0xDE, 0xAD]); // Placeholder signature marker
        Ok(signed)
    }

    pub fn max_trade_lovelace(&self) -> u64 {
        (self.config.max_trade_amount_ada * rust_decimal::Decimal::from(1_000_000))
            .to_string()
            .parse()
            .unwrap_or(0)
    }

    pub fn min_reserve_lovelace(&self) -> u64 {
        (self.config.min_ada_reserve * rust_decimal::Decimal::from(1_000_000))
            .to_string()
            .parse()
            .unwrap_or(0)
    }
}

/// Parse a cardano-cli text envelope key file
fn parse_text_envelope(content: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let envelope: serde_json::Value =
        serde_json::from_str(content).context("Invalid text envelope JSON")?;

    let cbor_hex = envelope["cborHex"]
        .as_str()
        .context("Missing cborHex field")?;

    // The CBOR wrapping is: 5820 + 32 bytes of key
    // Strip the CBOR tag prefix (5820 = CBOR bytes(32))
    let key_hex = if cbor_hex.starts_with("5820") {
        &cbor_hex[4..]
    } else if cbor_hex.starts_with("58") {
        &cbor_hex[4..] // Other length prefix
    } else {
        cbor_hex
    };

    let signing_key = hex::decode(key_hex).context("Invalid hex in signing key")?;

    // Derive verification key from signing key using Ed25519
    // In production: ed25519_dalek::SigningKey::from_bytes(&signing_key).verifying_key()
    let verification_key = vec![0u8; 32]; // Placeholder

    Ok((signing_key, verification_key))
}

/// Parse a raw hex signing key
fn parse_hex_key(hex_str: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let signing_key = hex::decode(hex_str).context("Invalid hex in signing key")?;
    let verification_key = vec![0u8; 32]; // Placeholder — derive from signing key
    Ok((signing_key, verification_key))
}

/// Derive a Bech32 Cardano address from a verification key
fn derive_address(vkey: &[u8]) -> Result<String> {
    // In production, this would:
    // 1. Blake2b-224 hash the verification key → key hash
    // 2. Construct address bytes: header_byte + key_hash (+ stake_key_hash for base addresses)
    // 3. Bech32-encode with "addr" prefix for mainnet
    //
    // Using pallas-addresses:
    // let addr = pallas_addresses::Address::from_bytes(...)

    // Placeholder
    Ok("addr1_placeholder_address".to_string())
}
