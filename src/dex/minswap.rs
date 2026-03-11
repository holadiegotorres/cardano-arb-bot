//! Minswap DEX Connector
//!
//! Minswap is the largest AMM DEX on Cardano by TVL.
//! It uses constant-product (x*y=k) pools with 0.3% swap fee and
//! also supports StableSwap pools for stablecoin pairs.
//!
//! Pools are identified by a unique NFT token held in the pool UTXO.
//! Swap orders are submitted to the order validator address with a
//! specific datum structure encoding the desired swap parameters.

use anyhow::{Context, Result};
use tracing::{debug, trace};

use super::types::*;
use crate::config::{BlockfrostConfig, DexConfig};

/// Decode a Bech32 Cardano address to raw address bytes
fn decode_bech32_to_raw(addr: &str) -> Result<Vec<u8>> {
    if addr.starts_with("addr") {
        let (_hrp, data) = bech32::decode(addr)
            .context("Invalid Bech32 address")?;
        Ok(data)
    } else {
        // Hex-encoded address bytes
        hex::decode(addr).context("Invalid hex address")
    }
}

pub struct MinswapConnector {
    config: DexConfig,
    http_client: reqwest::Client,
    blockfrost_key: String,
    blockfrost_url: String,
}

impl MinswapConnector {
    pub fn new(config: DexConfig, bf_config: &BlockfrostConfig) -> Result<Self> {
        let blockfrost_url = bf_config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://cardano-mainnet.blockfrost.io/api/v0".to_string());

        Ok(Self {
            config,
            http_client: reqwest::Client::new(),
            blockfrost_key: bf_config.api_key.clone(),
            blockfrost_url,
        })
    }

    /// Query Blockfrost for all UTXOs at the Minswap pool script address
    async fn fetch_pool_utxos(&self) -> Result<Vec<serde_json::Value>> {
        let url = format!(
            "{}/addresses/{}/utxos",
            self.blockfrost_url, self.config.pool_script_hash
        );

        let resp = self
            .http_client
            .get(&url)
            .header("project_id", &self.blockfrost_key)
            .send()
            .await
            .context("Failed to fetch Minswap pool UTXOs from Blockfrost")?;

        let utxos: Vec<serde_json::Value> = resp.json().await?;
        Ok(utxos)
    }

    /// Parse a Blockfrost UTXO response into a PoolState
    fn parse_pool_utxo(&self, utxo: &serde_json::Value) -> Result<PoolState> {
        let tx_hash = utxo["tx_hash"].as_str().unwrap_or_default();
        let output_index = utxo["output_index"].as_u64().unwrap_or(0);
        let utxo_ref = format!("{}#{}", tx_hash, output_index);

        // Extract amounts from the UTXO
        let amounts = utxo["amount"]
            .as_array()
            .context("Missing amount array in UTXO")?;

        let mut ada_amount: u64 = 0;
        let mut token_asset = AssetId::ada();
        let mut token_amount: u64 = 0;
        let mut has_pool_nft = false;

        for amount in amounts {
            let unit = amount["unit"].as_str().unwrap_or("lovelace");
            let quantity: u64 = amount["quantity"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);

            if unit == "lovelace" {
                ada_amount = quantity;
            } else if unit.starts_with(&self.config.pool_nft_policy_id) {
                has_pool_nft = true;
            } else {
                // This is the paired token
                let (policy_id, asset_name) = if unit.len() > 56 {
                    (unit[..56].to_string(), unit[56..].to_string())
                } else {
                    (unit.to_string(), String::new())
                };
                token_asset = AssetId {
                    policy_id,
                    asset_name,
                };
                token_amount = quantity;
            }
        }

        if !has_pool_nft {
            anyhow::bail!("UTXO does not contain pool NFT — not a valid pool");
        }

        Ok(PoolState {
            pool_id: utxo_ref.clone(),
            dex_id: DexId::Minswap,
            asset_a: AssetId::ada(),
            asset_b: token_asset,
            reserve_a: ada_amount,
            reserve_b: token_amount,
            lp_supply: 0, // Would need to decode datum for this
            fee: rust_decimal::Decimal::new(3, 3), // 0.3%
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            utxo_ref,
            pool_type: PoolType::ConstantProduct,
        })
    }
}

#[async_trait::async_trait]
impl super::DexConnector for MinswapConnector {
    fn dex_id(&self) -> DexId {
        DexId::Minswap
    }

    fn name(&self) -> &str {
        "Minswap"
    }

    async fn fetch_pools(&self) -> Result<Vec<PoolState>> {
        debug!("Fetching Minswap pools...");
        let utxos = self.fetch_pool_utxos().await?;

        let mut pools = Vec::new();
        for utxo in &utxos {
            match self.parse_pool_utxo(utxo) {
                Ok(pool) => {
                    trace!("Parsed pool: {} ({}/{})", pool.pool_id, pool.asset_a, pool.asset_b);
                    pools.push(pool);
                }
                Err(e) => {
                    trace!("Skipping UTXO (not a valid pool): {}", e);
                }
            }
        }

        debug!("Found {} valid Minswap pools", pools.len());
        Ok(pools)
    }

    async fn fetch_pool(&self, pool_id: &str) -> Result<PoolState> {
        // Parse tx_hash#index from pool_id
        let parts: Vec<&str> = pool_id.split('#').collect();
        anyhow::ensure!(parts.len() == 2, "Invalid pool_id format");

        let url = format!(
            "{}/txs/{}/utxos",
            self.blockfrost_url, parts[0]
        );

        let resp = self
            .http_client
            .get(&url)
            .header("project_id", &self.blockfrost_key)
            .send()
            .await?;

        let tx_utxos: serde_json::Value = resp.json().await?;
        let idx: usize = parts[1].parse()?;

        let output = &tx_utxos["outputs"][idx];
        self.parse_pool_utxo(output)
    }

    fn build_swap_order(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
        min_output: u64,
        receiver_address: &str,
    ) -> Result<SwapOrder> {
        // Determine swap direction: A→B or B→A
        let a_to_b = input_asset == "lovelace" || input_asset == pool.asset_a.to_subject();

        // Build the Minswap V2 SwapExactIn datum using our datum encoder
        let receiver_raw = decode_bech32_to_raw(receiver_address)?;

        let datum_data = crate::datum::minswap::build_swap_exact_in_datum(
            &receiver_raw,   // sender = receiver for arb bot (refunds go to us)
            &receiver_raw,   // receiver = our wallet
            a_to_b,
            min_output,
            self.config.batcher_fee_lovelace,
            2_000_000,       // output ADA (min UTXO)
        )?;
        let datum = datum_data.to_cbor()?;

        let value_lovelace = if input_asset == "lovelace" {
            input_amount + self.config.batcher_fee_lovelace + 2_000_000 // min ADA for UTXO
        } else {
            self.config.batcher_fee_lovelace + 2_000_000
        };

        let mut value_assets = Vec::new();
        if input_asset != "lovelace" {
            let parts: Vec<&str> = input_asset.split('.').collect();
            if parts.len() == 2 {
                value_assets.push((
                    AssetId {
                        policy_id: parts[0].to_string(),
                        asset_name: parts[1].to_string(),
                    },
                    input_amount,
                ));
            }
        }

        Ok(SwapOrder {
            dex_id: DexId::Minswap,
            pool_id: pool.pool_id.clone(),
            datum,
            order_address: self.config.order_script_hash.clone(),
            value_lovelace,
            value_assets,
            min_output,
        })
    }

    fn swap_fee(&self) -> f64 {
        0.003 // 0.3%
    }

    fn batcher_fee_lovelace(&self) -> u64 {
        self.config.batcher_fee_lovelace
    }

    fn calculate_output(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
    ) -> Result<u64> {
        let input_is_a = input_asset == "lovelace" || input_asset == pool.asset_a.to_subject();
        Ok(pool.calc_constant_product_output(input_amount, input_is_a))
    }
}
