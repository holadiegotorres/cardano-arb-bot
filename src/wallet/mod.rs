//! Wallet Manager
//!
//! Handles key management, address derivation, and transaction signing.
//! Uses Ed25519 for signing and Blake2b for hashing — the standard Cardano
//! cryptographic primitives.
//!
//! Key formats supported:
//!   1. cardano-cli text envelope (JSON with cborHex field)
//!   2. Raw hex Ed25519 secret key (32 bytes)

use anyhow::{Context, Result};
use blake2b_simd::Params as Blake2bParams;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use tracing::info;

use crate::config::WalletConfig;

pub struct WalletManager {
    /// The Ed25519 signing key
    signing_key: SigningKey,
    /// The verification (public) key
    verifying_key: VerifyingKey,
    /// Blake2b-224 hash of the verification key (28 bytes)
    key_hash: [u8; 28],
    /// Raw address bytes (for datum encoding)
    address_bytes: Vec<u8>,
    /// Bech32-encoded address
    address: String,
    /// Configuration
    config: WalletConfig,
    /// Network ID (0 = testnet, 1 = mainnet)
    network_id: u8,
}

impl WalletManager {
    pub fn new(config: &WalletConfig) -> Result<Self> {
        let key_content = std::fs::read_to_string(&config.signing_key_path)
            .with_context(|| {
                format!(
                    "Failed to read signing key from: {}",
                    config.signing_key_path
                )
            })?;

        let key_content = key_content.trim();

        // Parse key — supports both text envelope and raw hex formats
        let secret_bytes = if key_content.starts_with('{') {
            parse_text_envelope(key_content)?
        } else {
            parse_hex_key(key_content)?
        };

        anyhow::ensure!(
            secret_bytes.len() == 32,
            "Ed25519 signing key must be exactly 32 bytes, got {}",
            secret_bytes.len()
        );

        // Create Ed25519 signing key
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&secret_bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        // Compute Blake2b-224 of the verification key -> key hash
        let key_hash = blake2b_224(verifying_key.as_bytes());

        // Default to mainnet (network_id = 1)
        let network_id = 1u8;

        // Build enterprise address (type 6): no staking credential
        // Header byte: 0110_NNNN where NNNN = network_id
        let header_byte = 0x60 | (network_id & 0x0F);
        let mut address_bytes = Vec::with_capacity(29);
        address_bytes.push(header_byte);
        address_bytes.extend_from_slice(&key_hash);

        // Bech32-encode the address
        let address = bech32_encode_address(&address_bytes, network_id)?;

        info!(
            "Wallet initialized | address: {} | key_hash: {} | min_reserve: {} ADA | max_trade: {} ADA",
            address,
            hex::encode(&key_hash),
            config.min_ada_reserve,
            config.max_trade_amount_ada
        );

        Ok(Self {
            signing_key,
            verifying_key,
            key_hash,
            address_bytes,
            address,
            config: config.clone(),
            network_id,
        })
    }

    pub fn address_bech32(&self) -> &str {
        &self.address
    }

    /// Get raw address bytes (for encoding into datums)
    pub fn address_bytes(&self) -> &[u8] {
        &self.address_bytes
    }

    /// Get the payment key hash (Blake2b-224 of vkey, 28 bytes)
    pub fn payment_key_hash(&self) -> &[u8; 28] {
        &self.key_hash
    }

    /// Get the verification key bytes (32 bytes)
    pub fn verification_key_bytes(&self) -> &[u8] {
        self.verifying_key.as_bytes()
    }

    /// Sign a transaction body (CBOR bytes).
    ///
    /// Cardano transaction signing:
    /// 1. Blake2b-256 hash the transaction body
    /// 2. Ed25519-sign the hash
    /// 3. Construct the full signed transaction:
    ///    CBOR Array(3): [tx_body, witness_set, true]
    ///    where witness_set = { 0: [[vkey, signature]] }
    pub fn sign_transaction(&self, tx_body_cbor: &[u8]) -> Result<Vec<u8>> {
        // Step 1: Blake2b-256 hash of the tx body
        let tx_hash = blake2b_256(tx_body_cbor);

        // Step 2: Ed25519 sign the hash
        let signature = self.signing_key.sign(&tx_hash);

        // Step 3: Build the full signed transaction as CBOR
        let vkey_bytes = self.verifying_key.as_bytes();
        let sig_bytes = signature.to_bytes();

        let mut signed_tx = Vec::new();

        // CBOR array(3) header
        signed_tx.push(0x83);

        // Element 0: tx body (already CBOR-encoded)
        signed_tx.extend_from_slice(tx_body_cbor);

        // Element 1: witness set map(1) { 0: [ [vkey(32), sig(64)] ] }
        signed_tx.push(0xA1); // map(1)
        signed_tx.push(0x00); // key: 0

        signed_tx.push(0x81); // array(1) — one vkey witness

        signed_tx.push(0x82); // array(2) — [vkey, sig]

        // vkey: bytes(32)
        signed_tx.push(0x58);
        signed_tx.push(0x20);
        signed_tx.extend_from_slice(vkey_bytes);

        // signature: bytes(64)
        signed_tx.push(0x58);
        signed_tx.push(0x40);
        signed_tx.extend_from_slice(&sig_bytes);

        // Element 2: is_valid = true
        signed_tx.push(0xF5); // CBOR true

        Ok(signed_tx)
    }

    /// Get the Blake2b-256 transaction hash (for tracking)
    pub fn tx_body_hash(tx_body_cbor: &[u8]) -> [u8; 32] {
        blake2b_256(tx_body_cbor)
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

    pub fn network_id(&self) -> u8 {
        self.network_id
    }

    /// Set network (call during init if config says preprod/preview)
    pub fn set_network_testnet(&mut self) -> Result<()> {
        self.network_id = 0;
        let header_byte = 0x60 | (self.network_id & 0x0F);
        self.address_bytes[0] = header_byte;
        self.address = bech32_encode_address(&self.address_bytes, self.network_id)?;
        Ok(())
    }
}

// ---- Key Parsing ----

/// Parse a cardano-cli text envelope key file
fn parse_text_envelope(content: &str) -> Result<Vec<u8>> {
    let envelope: serde_json::Value =
        serde_json::from_str(content).context("Invalid text envelope JSON")?;

    let cbor_hex = envelope["cborHex"]
        .as_str()
        .context("Missing cborHex field in key envelope")?;

    // cardano-cli wraps the key in CBOR: 5820 + 32 bytes
    let key_hex = if cbor_hex.starts_with("5820") && cbor_hex.len() == 68 {
        &cbor_hex[4..]
    } else if cbor_hex.starts_with("58") && cbor_hex.len() >= 6 {
        let len_byte = u8::from_str_radix(&cbor_hex[2..4], 16).unwrap_or(32) as usize;
        let data_hex = &cbor_hex[4..];
        if len_byte > 32 && data_hex.len() >= 64 {
            &data_hex[..64]
        } else {
            data_hex
        }
    } else {
        cbor_hex
    };

    let signing_key = hex::decode(key_hex).context("Invalid hex in signing key")?;

    if signing_key.len() > 32 {
        Ok(signing_key[..32].to_vec())
    } else {
        Ok(signing_key)
    }
}

/// Parse a raw hex signing key
fn parse_hex_key(hex_str: &str) -> Result<Vec<u8>> {
    let bytes = hex::decode(hex_str.trim()).context("Invalid hex in signing key")?;
    if bytes.len() > 32 {
        Ok(bytes[..32].to_vec())
    } else {
        Ok(bytes)
    }
}

// ---- Blake2b hashing ----

/// Blake2b-224 (28 bytes) — used for key hashes and script hashes
fn blake2b_224(data: &[u8]) -> [u8; 28] {
    let hash = Blake2bParams::new()
        .hash_length(28)
        .hash(data);
    let mut out = [0u8; 28];
    out.copy_from_slice(hash.as_bytes());
    out
}

/// Blake2b-256 (32 bytes) — used for transaction hashes
fn blake2b_256(data: &[u8]) -> [u8; 32] {
    let hash = Blake2bParams::new()
        .hash_length(32)
        .hash(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    out
}

// ---- Bech32 address encoding ----

/// Encode raw address bytes as Bech32 with the appropriate HRP
fn bech32_encode_address(addr_bytes: &[u8], network_id: u8) -> Result<String> {
    let hrp = if network_id == 1 {
        bech32::Hrp::parse("addr").context("Invalid HRP")?
    } else {
        bech32::Hrp::parse("addr_test").context("Invalid HRP")?
    };

    let encoded = bech32::encode::<bech32::Bech32>(hrp, addr_bytes)
        .context("Bech32 encoding failed")?;

    Ok(encoded)
}
