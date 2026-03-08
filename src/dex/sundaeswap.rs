//! SundaeSwap DEX Connector
//!
//! SundaeSwap was one of the first AMMs on Cardano. V2 uses concentrated
//! liquidity for improved capital efficiency. Swap fee is typically 0.3%.
//! Orders are submitted as UTXOs to the order validator with a specific datum.

use anyhow::{Context, Result};
use tracing::debug;

use super::types::*;
use crate::config::{BlockfrostConfig, DexConfig};

pub struct SundaeSwapConnector {
    config: DexConfig,
    http_client: reqwest::Client,
    blockfrost_key: String,
    blockfrost_url: String,
}

impl SundaeSwapConnector {
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
            .context("Failed to fetch SundaeSwap pool UTXOs")?;

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
                token_asset = AssetId {
                    policy_id,
                    asset_name,
                };
                token_amount = quantity;
            }
        }

        Ok(PoolState {
            pool_id: utxo_ref.clone(),
            dex_id: DexId::SundaeSwap,
            asset_a: AssetId::ada(),
            asset_b: token_asset,
            reserve_a: ada_amount,
            reserve_b: token_amount,
            lp_supply: 0,
            fee: rust_decimal::Decimal::new(3, 3),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            utxo_ref,
            pool_type: PoolType::ConstantProduct,
        })
    }
}

#[async_trait::async_trait]
impl super::DexConnector for SundaeSwapConnector {
    fn dex_id(&self) -> DexId {
        DexId::SundaeSwap
    }

    fn name(&self) -> &str {
        "SundaeSwap"
    }

    async fn fetch_pools(&self) -> Result<Vec<PoolState>> {
        debug!("Fetching SundaeSwap pools...");
        let utxos = self.fetch_pool_utxos().await?;

        let mut pools = Vec::new();
        for utxo in &utxos {
            if let Ok(pool) = self.parse_pool_utxo(utxo) {
                pools.push(pool);
            }
        }

        debug!("Found {} valid SundaeSwap pools", pools.len());
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
        let datum = Vec::new(); // TODO: SundaeSwap V2 datum encoding

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
            dex_id: DexId::SundaeSwap,
            pool_id: pool.pool_id.clone(),
            datum,
            order_address: self.config.order_script_hash.clone(),
            value_lovelace,
            value_assets,
            min_output,
        })
    }

    fn swap_fee(&self) -> f64 {
        0.003
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
