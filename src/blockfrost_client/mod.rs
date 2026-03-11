//! Blockfrost API Client
//!
//! Handles all interactions with the Blockfrost REST API for querying
//! the Cardano blockchain. Uses reqwest with rate limiting.
//!
//! Key endpoints:
//!   GET /addresses/{address}/utxos  — Fetch UTXOs at an address
//!   POST /tx/submit                 — Submit a signed transaction (CBOR binary)
//!   GET /txs/{hash}                 — Check if a transaction is confirmed
//!   GET /epochs/latest/parameters   — Current protocol parameters

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, trace, warn};

use crate::config::BlockfrostConfig;

/// Wrapper around the Blockfrost REST API
pub struct BlockfrostClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    rate_limit_rps: u32,
}

/// A UTXO as returned by Blockfrost
#[derive(Debug, Clone, Deserialize)]
pub struct BlockfrostUtxo {
    pub tx_hash: String,
    pub output_index: u64,
    pub amount: Vec<BlockfrostAmount>,
    pub block: Option<String>,
    pub data_hash: Option<String>,
    pub inline_datum: Option<String>,
    pub reference_script_hash: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockfrostAmount {
    pub unit: String,
    pub quantity: String,
}

/// Protocol parameters from Blockfrost
#[derive(Debug, Clone, Deserialize)]
pub struct ProtocolParameters {
    pub min_fee_a: u64,
    pub min_fee_b: u64,
    pub max_tx_size: u64,
    pub key_deposit: String,
    pub pool_deposit: String,
    pub min_utxo: Option<String>,
    pub coins_per_utxo_size: Option<String>,
    pub coins_per_utxo_word: Option<String>,
    pub collateral_percent: Option<u64>,
    pub max_collateral_inputs: Option<u64>,
    pub price_mem: Option<f64>,
    pub price_step: Option<f64>,
}

/// Transaction status from Blockfrost
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionInfo {
    pub hash: String,
    pub block: Option<String>,
    pub block_height: Option<u64>,
    pub block_time: Option<u64>,
    pub index: Option<u64>,
    pub fees: Option<String>,
    pub size: Option<u64>,
    pub valid_contract: Option<bool>,
}

impl BlockfrostClient {
    pub fn new(config: &BlockfrostConfig) -> Self {
        let base_url = config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://cardano-mainnet.blockfrost.io/api/v0".to_string());

        Self {
            http: reqwest::Client::new(),
            base_url,
            api_key: config.api_key.clone(),
            rate_limit_rps: config.rate_limit_rps,
        }
    }

    /// Build a GET request with the project_id header
    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        self.http
            .get(&url)
            .header("project_id", &self.api_key)
    }

    /// Build a POST request with the project_id header
    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        self.http
            .post(&url)
            .header("project_id", &self.api_key)
    }

    // ──────────────────────────────────────────────
    //  UTXO queries
    // ──────────────────────────────────────────────

    /// Fetch all UTXOs at a given address (with pagination)
    pub async fn get_utxos(&self, address: &str) -> Result<Vec<BlockfrostUtxo>> {
        let mut all_utxos = Vec::new();
        let mut page = 1u32;

        loop {
            let path = format!("/addresses/{}/utxos?page={}&count=100", address, page);
            let resp = self
                .get(&path)
                .send()
                .await
                .context("Blockfrost UTXO request failed")?;

            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                // Address has no UTXOs
                break;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!(
                    "Blockfrost error {} fetching UTXOs for {}: {}",
                    status,
                    address,
                    body
                );
            }

            let utxos: Vec<BlockfrostUtxo> = resp.json().await?;
            let count = utxos.len();
            all_utxos.extend(utxos);

            if count < 100 {
                break; // Last page
            }
            page += 1;
        }

        trace!("Fetched {} UTXOs for {}", all_utxos.len(), address);
        Ok(all_utxos)
    }

    /// Fetch UTXOs at a script address (used for pool scanning)
    pub async fn get_script_utxos(&self, script_address: &str) -> Result<Vec<serde_json::Value>> {
        let mut all = Vec::new();
        let mut page = 1u32;

        loop {
            let path = format!(
                "/addresses/{}/utxos?page={}&count=100",
                script_address, page
            );
            let resp = self.get(&path).send().await?;

            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                break;
            }
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Blockfrost error {}: {}", status, body);
            }

            let utxos: Vec<serde_json::Value> = resp.json().await?;
            let count = utxos.len();
            all.extend(utxos);

            if count < 100 {
                break;
            }
            page += 1;
        }

        Ok(all)
    }

    // ──────────────────────────────────────────────
    //  Transaction submission
    // ──────────────────────────────────────────────

    /// Submit a signed transaction (raw CBOR bytes) to the network
    ///
    /// Returns the transaction hash on success.
    /// Blockfrost expects Content-Type: application/cbor with raw binary body.
    pub async fn submit_transaction(&self, signed_tx_cbor: &[u8]) -> Result<String> {
        debug!(
            "Submitting transaction ({} bytes) to Blockfrost",
            signed_tx_cbor.len()
        );

        let resp = self
            .post("/tx/submit")
            .header("Content-Type", "application/cbor")
            .body(signed_tx_cbor.to_vec())
            .send()
            .await
            .context("Blockfrost tx submit request failed")?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!(
                "Transaction submission failed (HTTP {}): {}",
                status,
                body
            );
        }

        // Blockfrost returns the tx hash as a JSON string (quoted)
        let tx_hash = body.trim().trim_matches('"').to_string();
        debug!("Transaction submitted successfully: {}", tx_hash);
        Ok(tx_hash)
    }

    // ──────────────────────────────────────────────
    //  Transaction confirmation
    // ──────────────────────────────────────────────

    /// Check if a transaction has been confirmed on-chain.
    /// Returns Some(info) if confirmed, None if not yet found.
    pub async fn get_transaction(&self, tx_hash: &str) -> Result<Option<TransactionInfo>> {
        let path = format!("/txs/{}", tx_hash);
        let resp = self.get(&path).send().await?;

        match resp.status() {
            s if s.is_success() => {
                let info: TransactionInfo = resp.json().await?;
                Ok(Some(info))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Blockfrost error checking tx {}: {} {}", tx_hash, status, body);
            }
        }
    }

    // ──────────────────────────────────────────────
    //  Protocol parameters
    // ──────────────────────────────────────────────

    /// Fetch current epoch protocol parameters
    pub async fn get_protocol_parameters(&self) -> Result<ProtocolParameters> {
        let resp = self
            .get("/epochs/latest/parameters")
            .send()
            .await
            .context("Failed to fetch protocol parameters")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Blockfrost protocol params error: {}", body);
        }

        let params: ProtocolParameters = resp.json().await?;
        Ok(params)
    }

    // ──────────────────────────────────────────────
    //  Helpers
    // ──────────────────────────────────────────────

    /// Parse lovelace amount from a Blockfrost UTXO
    pub fn utxo_lovelace(utxo: &BlockfrostUtxo) -> u64 {
        utxo.amount
            .iter()
            .find(|a| a.unit == "lovelace")
            .and_then(|a| a.quantity.parse::<u64>().ok())
            .unwrap_or(0)
    }

    /// Parse all native assets from a Blockfrost UTXO (excluding lovelace)
    pub fn utxo_assets(utxo: &BlockfrostUtxo) -> Vec<(String, u64)> {
        utxo.amount
            .iter()
            .filter(|a| a.unit != "lovelace")
            .filter_map(|a| {
                let qty = a.quantity.parse::<u64>().ok()?;
                Some((a.unit.clone(), qty))
            })
            .collect()
    }

    /// Get the current slot tip (for TTL calculation)
    pub async fn get_latest_block_slot(&self) -> Result<u64> {
        let resp = self.get("/blocks/latest").send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch latest block");
        }

        let block: serde_json::Value = resp.json().await?;
        let slot = block["slot"]
            .as_u64()
            .context("Missing slot in latest block")?;
        Ok(slot)
    }
}
