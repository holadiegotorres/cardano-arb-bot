//! WingRiders DEX Connector
//!
//! WingRiders is known for strong stablecoin support (StableSwap pools)
//! and has never been hacked. Audited by CertiK and PeckShield.
//! Particularly interesting for USDCx ↔ DJED ↔ iUSD ↔ USDT pairs
//! where slippage is extremely low (<0.1%).

use anyhow::{Context, Result};
use tracing::debug;

use super::types::*;
use crate::config::{BlockfrostConfig, DexConfig};

pub struct WingRidersConnector {
    config: DexConfig,
    http_client: reqwest::Client,
    blockfrost_key: String,
    blockfrost_url: String,
}

impl WingRidersConnector {
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
            .context("Failed to fetch WingRiders pool UTXOs")?;

        Ok(resp.json().await?)
    }

    fn parse_pool_utxo(&self, utxo: &serde_json::Value) -> Result<PoolState> {
        let tx_hash = utxo["tx_hash"].as_str().unwrap_or_default();
        let output_index = utxo["output_index"].as_u64().unwrap_or(0);
        let utxo_ref = format!("{}#{}", tx_hash, output_index);

        let amounts = utxo["amount"]
            .as_array()
            .context("Missing amount array")?;

        let mut ada_amount: u64 = 0;
        let mut token_asset = AssetId::ada();
        let mut token_amount: u64 = 0;
        let mut is_stableswap = false;

        for amount in amounts {
            let unit = amount["unit"].as_str().unwrap_or("lovelace");
            let quantity: u64 = amount["quantity"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);

            if unit == "lovelace" {
                ada_amount = quantity;
            } else if !unit.starts_with(&self.config.pool_nft_policy_id) {
                let (policy_id, asset_name) = if unit.len() > 56 {
                    (unit[..56].to_string(), unit[56..].to_string())
                } else {
                    (unit.to_string(), String::new())
                };

                // Detect if this is a stablecoin pair
                let known_stables = ["DJED", "iUSD", "USDCx", "USDT"];
                let decoded_name = hex::decode(&asset_name)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_default();

                if known_stables.iter().any(|s| decoded_name.contains(s)) {
                    is_stableswap = true;
                }

                token_asset = AssetId {
                    policy_id,
                    asset_name,
                };
                token_amount = quantity;
            }
        }

        let pool_type = if is_stableswap {
            PoolType::StableSwap { amp: 100 } // WingRiders typical amp factor
        } else {
            PoolType::ConstantProduct
        };

        Ok(PoolState {
            pool_id: utxo_ref.clone(),
            dex_id: DexId::WingRiders,
            asset_a: AssetId::ada(),
            asset_b: token_asset,
            reserve_a: ada_amount,
            reserve_b: token_amount,
            lp_supply: 0,
            fee: rust_decimal::Decimal::new(35, 4), // 0.35%
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            utxo_ref,
            pool_type,
        })
    }
}

#[async_trait::async_trait]
impl super::DexConnector for WingRidersConnector {
    fn dex_id(&self) -> DexId {
        DexId::WingRiders
    }

    fn name(&self) -> &str {
        "WingRiders"
    }

    async fn fetch_pools(&self) -> Result<Vec<PoolState>> {
        debug!("Fetching WingRiders pools...");
        let utxos = self.fetch_pool_utxos().await?;

        let mut pools = Vec::new();
        for utxo in &utxos {
            if let Ok(pool) = self.parse_pool_utxo(utxo) {
                pools.push(pool);
            }
        }

        debug!("Found {} valid WingRiders pools", pools.len());
        Ok(pools)
    }

    async fn fetch_pool(&self, pool_id: &str) -> Result<PoolState> {
        let parts: Vec<&str> = pool_id.split('#').collect();
        anyhow::ensure!(parts.len() == 2, "Invalid pool_id format");

        let url = format!("{}/txs/{}/utxos", self.blockfrost_url, parts[0]);
        let resp = self
            .http_client
            .get(&url)
            .header("project_id", &self.blockfrost_key)
            .send()
            .await?;

        let tx_utxos: serde_json::Value = resp.json().await?;
        let idx: usize = parts[1].parse()?;
        self.parse_pool_utxo(&tx_utxos["outputs"][idx])
    }

    fn build_swap_order(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
        min_output: u64,
        _receiver_address: &str,
    ) -> Result<SwapOrder> {
        let datum = Vec::new(); // TODO: WingRiders datum encoding

        let value_lovelace = if input_asset == "lovelace" {
            input_amount + self.config.batcher_fee_lovelace + 2_000_000
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
            dex_id: DexId::WingRiders,
            pool_id: pool.pool_id.clone(),
            datum,
            order_address: self.config.order_script_hash.clone(),
            value_lovelace,
            value_assets,
            min_output,
        })
    }

    fn swap_fee(&self) -> f64 {
        0.0035 // 0.35%
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

        match &pool.pool_type {
            PoolType::StableSwap { amp } => {
                // StableSwap invariant: An^n * sum(x_i) + D = A * D^n * n^n + D^(n+1) / (n^n * prod(x_i))
                // Simplified for 2 tokens: nearly 1:1 at balanced reserves
                let (reserve_in, reserve_out) = if input_is_a {
                    (pool.reserve_a as f64, pool.reserve_b as f64)
                } else {
                    (pool.reserve_b as f64, pool.reserve_a as f64)
                };

                let a = *amp as f64;
                let input = input_amount as f64;

                // Newton's method approximation for StableSwap output
                let d = reserve_in + reserve_out;
                let new_reserve_in = reserve_in + input;
                // For stableswap with high amp, output ≈ input (minus fees)
                let output_approx = reserve_out - (d - new_reserve_in)
                    * (1.0 - 1.0 / (4.0 * a));

                let after_fee = output_approx * (1.0 - 0.0035);
                Ok(after_fee.max(0.0) as u64)
            }
            _ => Ok(pool.calc_constant_product_output(input_amount, input_is_a)),
        }
    }
}
