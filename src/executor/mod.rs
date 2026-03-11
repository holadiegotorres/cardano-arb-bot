//! Transaction Executor
//!
//! Takes an ArbOpportunity and turns it into real Cardano transactions.
//! Handles UTXO selection, transaction building (raw CBOR), signing,
//! submission via Blockfrost, and confirmation polling.
//!
//! For Cardano DEX arb, each swap creates an order UTXO at the DEX's
//! order validator address. The DEX batcher then fulfills the order.

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::blockfrost_client::{BlockfrostClient, BlockfrostUtxo};
use crate::config::ExecutorConfig;
use crate::dex::types::*;
use crate::dex::DexRegistry;
use crate::wallet::WalletManager;

pub struct TransactionExecutor {
    wallet: Arc<WalletManager>,
    dex_registry: Arc<DexRegistry>,
    blockfrost: BlockfrostClient,
    config: ExecutorConfig,
    dry_run: bool,
    /// Track recent executions to avoid double-executing
    recent_tx_hashes: Vec<String>,
}

impl TransactionExecutor {
    pub fn new(
        wallet: Arc<WalletManager>,
        dex_registry: Arc<DexRegistry>,
        blockfrost: BlockfrostClient,
        config: ExecutorConfig,
        dry_run: bool,
    ) -> Self {
        Self {
            wallet,
            dex_registry,
            blockfrost,
            config,
            dry_run,
            recent_tx_hashes: Vec::new(),
        }
    }

    /// Execute an arbitrage opportunity
    pub async fn execute(&self, opportunity: &ArbOpportunity) -> Result<String> {
        info!(
            "Executing {} opportunity: {}",
            opportunity.strategy_type, opportunity.route_description
        );

        if self.dry_run {
            info!("[DRY-RUN] Would execute: {}", opportunity.route_description);
            info!(
                "[DRY-RUN] Input: {} lovelace | Expected profit: {} ADA",
                opportunity.input_amount, opportunity.estimated_profit_ada
            );
            return Ok("DRY_RUN_TX_HASH".to_string());
        }

        // Execute each step in the arb route
        let mut last_tx_hash = String::new();

        for (i, step) in opportunity.steps.iter().enumerate() {
            info!(
                "Step {}/{}: {} on {} ({} -> {})",
                i + 1,
                opportunity.steps.len(),
                step.dex_id,
                step.pool_id,
                step.input_asset,
                step.output_asset
            );

            match self.execute_step(step).await {
                Ok(tx_hash) => {
                    info!("Step {} submitted: {}", i + 1, tx_hash);
                    last_tx_hash = tx_hash;

                    // Wait for confirmation before next step (multi-step arb)
                    if i < opportunity.steps.len() - 1 {
                        self.wait_for_confirmation(&last_tx_hash).await?;
                    }
                }
                Err(e) => {
                    error!("Step {} failed: {}", i + 1, e);
                    return Err(e);
                }
            }
        }

        Ok(last_tx_hash)
    }

    /// Execute a single swap step
    async fn execute_step(&self, step: &ArbStep) -> Result<String> {
        let connector = self
            .dex_registry
            .get(&step.dex_id)
            .context("DEX connector not found")?;

        // Re-fetch current pool state for accurate reserves
        let pool = connector.fetch_pool(&step.pool_id).await
            .context("Failed to refresh pool state before execution")?;

        // Build the swap order (includes datum encoding)
        let order = connector.build_swap_order(
            &pool,
            &step.input_asset.to_subject(),
            step.input_amount,
            step.min_output,
            self.wallet.address_bech32(),
        )?;

        // Build the transaction
        let tx_body = self.build_transaction(&order).await?;

        // Sign the transaction
        let signed_tx = self.wallet.sign_transaction(&tx_body)?;

        // Compute tx hash for logging
        let tx_hash_bytes = WalletManager::tx_body_hash(&tx_body);
        let tx_hash = hex::encode(&tx_hash_bytes);
        debug!("Transaction hash: {}", tx_hash);

        // Submit to network
        let submitted_hash = self.blockfrost.submit_transaction(&signed_tx).await?;

        Ok(submitted_hash)
    }

    /// Build a transaction body that creates a swap order UTXO.
    ///
    /// Cardano Babbage-era transaction body is a CBOR map with these fields:
    ///   0: inputs     — set of [tx_hash(32), output_index]
    ///   1: outputs    — array of [address, value, datum_option]
    ///   2: fee        — integer (lovelace)
    ///   3: ttl        — integer (slot number)
    async fn build_transaction(&self, order: &SwapOrder) -> Result<Vec<u8>> {
        // 1. Fetch wallet UTXOs for input selection
        let wallet_addr = self.wallet.address_bech32();
        let utxos = self.blockfrost.get_utxos(wallet_addr).await
            .context("Failed to fetch wallet UTXOs")?;

        anyhow::ensure!(!utxos.is_empty(), "No UTXOs available in wallet");

        // 2. Calculate total value needed
        let total_lovelace_needed = order.value_lovelace + self.config.max_fee_lovelace;

        // 3. Simple UTXO selection: greedy, pick largest first
        let mut selected_utxos: Vec<&BlockfrostUtxo> = Vec::new();
        let mut selected_lovelace: u64 = 0;

        let mut sorted_utxos: Vec<&BlockfrostUtxo> = utxos.iter().collect();
        sorted_utxos.sort_by(|a, b| {
            BlockfrostClient::utxo_lovelace(b).cmp(&BlockfrostClient::utxo_lovelace(a))
        });

        for utxo in &sorted_utxos {
            if selected_lovelace >= total_lovelace_needed {
                break;
            }
            selected_utxos.push(utxo);
            selected_lovelace += BlockfrostClient::utxo_lovelace(utxo);
        }

        anyhow::ensure!(
            selected_lovelace >= total_lovelace_needed,
            "Insufficient ADA: have {} lovelace, need {}",
            selected_lovelace,
            total_lovelace_needed
        );

        // 4. Get current slot for TTL
        let current_slot = self.blockfrost.get_latest_block_slot().await
            .unwrap_or(0);
        let ttl = current_slot + self.config.tx_ttl_slots;

        // 5. Calculate fee and change
        let fee = self.config.max_fee_lovelace; // Use max fee; can optimize later
        let change_lovelace = selected_lovelace - order.value_lovelace - fee;

        // Ensure change output has minimum ADA (1.5 ADA)
        let min_change_ada: u64 = 1_500_000;
        anyhow::ensure!(
            change_lovelace >= min_change_ada || change_lovelace == 0,
            "Change output too small: {} lovelace (min {})",
            change_lovelace,
            min_change_ada
        );

        // 6. Build CBOR transaction body manually
        let mut tx_body = Vec::new();

        // Determine how many map entries we need
        // 0: inputs, 1: outputs, 2: fee, 3: ttl
        // If we have a datum, we also include datum in outputs
        let has_datum = !order.datum.is_empty();

        // Map header: map(4) for standard fields
        tx_body.push(0xA4);

        // --- Field 0: inputs ---
        tx_body.push(0x00); // key: 0
        encode_cbor_array_header(&mut tx_body, selected_utxos.len());
        for utxo in &selected_utxos {
            // Each input is array(2): [tx_hash_bytes, output_index]
            tx_body.push(0x82);
            // tx_hash: bytes(32)
            let hash_bytes = hex::decode(&utxo.tx_hash)
                .context("Invalid tx hash hex")?;
            encode_cbor_bytestring(&mut tx_body, &hash_bytes);
            // output_index: uint
            encode_cbor_unsigned(&mut tx_body, utxo.output_index);
        }

        // --- Field 1: outputs ---
        tx_body.push(0x01); // key: 1
        let num_outputs = if change_lovelace >= min_change_ada { 2 } else { 1 };
        encode_cbor_array_header(&mut tx_body, num_outputs);

        // Output 0: the swap order UTXO at the DEX order address
        if has_datum {
            // Babbage-era output with datum: map { 0: address, 1: value, 2: datum_option }
            tx_body.push(0xA3); // map(3)

            // 0: address
            tx_body.push(0x00);
            let order_addr_bytes = decode_bech32_address(&order.order_address)?;
            encode_cbor_bytestring(&mut tx_body, &order_addr_bytes);

            // 1: value
            tx_body.push(0x01);
            if order.value_assets.is_empty() {
                // Simple: just lovelace
                encode_cbor_unsigned(&mut tx_body, order.value_lovelace);
            } else {
                // Multi-asset: [lovelace, { policy: { asset: quantity } }]
                tx_body.push(0x82); // array(2)
                encode_cbor_unsigned(&mut tx_body, order.value_lovelace);
                // Build multi-asset map
                encode_cbor_map_header(&mut tx_body, order.value_assets.len());
                for (asset, qty) in &order.value_assets {
                    let policy_bytes = hex::decode(&asset.policy_id).unwrap_or_default();
                    encode_cbor_bytestring(&mut tx_body, &policy_bytes);
                    encode_cbor_map_header(&mut tx_body, 1);
                    let name_bytes = hex::decode(&asset.asset_name).unwrap_or_default();
                    encode_cbor_bytestring(&mut tx_body, &name_bytes);
                    encode_cbor_unsigned(&mut tx_body, *qty);
                }
            }

            // 2: datum_option — inline datum = [1, datum_cbor]
            tx_body.push(0x02);
            tx_body.push(0x82); // array(2)
            tx_body.push(0x01); // tag: inline datum
            // Wrap datum as CBOR tagged data (tag 24 = encoded CBOR)
            encode_cbor_tag(&mut tx_body, 24);
            encode_cbor_bytestring(&mut tx_body, &order.datum);
        } else {
            // Legacy output without datum: [address, value]
            tx_body.push(0x82); // array(2)
            let order_addr_bytes = decode_bech32_address(&order.order_address)?;
            encode_cbor_bytestring(&mut tx_body, &order_addr_bytes);
            encode_cbor_unsigned(&mut tx_body, order.value_lovelace);
        }

        // Output 1: change back to our wallet
        if change_lovelace >= min_change_ada {
            tx_body.push(0x82); // array(2)
            encode_cbor_bytestring(&mut tx_body, self.wallet.address_bytes());
            encode_cbor_unsigned(&mut tx_body, change_lovelace);
        }

        // --- Field 2: fee ---
        tx_body.push(0x02); // key: 2
        encode_cbor_unsigned(&mut tx_body, fee);

        // --- Field 3: ttl ---
        tx_body.push(0x03); // key: 3
        encode_cbor_unsigned(&mut tx_body, ttl);

        debug!(
            "Built tx body: {} inputs, {} outputs, fee={}, ttl={}, {} bytes",
            selected_utxos.len(),
            num_outputs,
            fee,
            ttl,
            tx_body.len()
        );

        Ok(tx_body)
    }

    /// Wait for a transaction to be confirmed on-chain
    async fn wait_for_confirmation(&self, tx_hash: &str) -> Result<()> {
        info!("Waiting for confirmation: {}", tx_hash);

        for attempt in 0..self.config.max_retries {
            tokio::time::sleep(tokio::time::Duration::from_millis(
                self.config.retry_delay_ms,
            ))
            .await;

            match self.blockfrost.get_transaction(tx_hash).await {
                Ok(Some(info)) => {
                    info!(
                        "Transaction confirmed: {} (block: {:?})",
                        tx_hash,
                        info.block
                    );
                    return Ok(());
                }
                Ok(None) => {
                    debug!(
                        "Confirmation check {}/{} for {} — not yet confirmed",
                        attempt + 1,
                        self.config.max_retries,
                        tx_hash
                    );
                }
                Err(e) => {
                    warn!("Error checking confirmation: {}", e);
                }
            }
        }

        anyhow::bail!(
            "Transaction not confirmed after {} attempts: {}",
            self.config.max_retries,
            tx_hash
        )
    }
}

// ---- CBOR encoding helpers ----

fn encode_cbor_tag(buf: &mut Vec<u8>, tag: u64) {
    encode_cbor_header(buf, 6, tag);
}

fn encode_cbor_unsigned(buf: &mut Vec<u8>, n: u64) {
    encode_cbor_header(buf, 0, n);
}

fn encode_cbor_bytestring(buf: &mut Vec<u8>, bytes: &[u8]) {
    encode_cbor_header(buf, 2, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn encode_cbor_array_header(buf: &mut Vec<u8>, len: usize) {
    encode_cbor_header(buf, 4, len as u64);
}

fn encode_cbor_map_header(buf: &mut Vec<u8>, len: usize) {
    encode_cbor_header(buf, 5, len as u64);
}

fn encode_cbor_header(buf: &mut Vec<u8>, major_type: u8, value: u64) {
    let mt = major_type << 5;
    if value < 24 {
        buf.push(mt | value as u8);
    } else if value <= 0xFF {
        buf.push(mt | 24);
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(mt | 25);
        buf.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= 0xFFFFFFFF {
        buf.push(mt | 26);
        buf.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        buf.push(mt | 27);
        buf.extend_from_slice(&value.to_be_bytes());
    }
}

/// Decode a Bech32 address string to raw bytes
fn decode_bech32_address(addr: &str) -> Result<Vec<u8>> {
    // Handle both bech32 addresses and raw hex script hashes
    if addr.starts_with("addr") {
        let (_hrp, data) = bech32::decode(addr)
            .context("Invalid Bech32 address")?;
        Ok(data)
    } else {
        // Assume it's a hex-encoded script hash — build a script address
        // Type 7 (script, no staking): header = 0x71 (mainnet) or 0x70 (testnet)
        let hash_bytes = hex::decode(addr)
            .context("Address is neither valid Bech32 nor valid hex")?;

        if hash_bytes.len() == 28 {
            // It's a script hash — build enterprise script address (mainnet)
            let mut addr_bytes = Vec::with_capacity(29);
            addr_bytes.push(0x71); // script enterprise address, mainnet
            addr_bytes.extend_from_slice(&hash_bytes);
            Ok(addr_bytes)
        } else {
            // Return raw bytes as-is
            Ok(hash_bytes)
        }
    }
}
