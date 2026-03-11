//! MuesliSwap DEX Connector
//!
//! MuesliSwap is a hybrid DEX with both AMM pools and an order book.
//! It provides a public REST API for querying pool data, which makes
//! it particularly easy to integrate.
//! API docs: https://docs.muesliswap.com/cardano/muesli-api

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

use super::types::*;
use crate::config::{BlockfrostConfig, DexConfig};

fn decode_bech32_to_raw(addr: &str) -> Result<Vec<u8>> {
    if addr.starts_with("addr") {
        let (_hrp, data) = bech32::decode(addr)
            .context("Invalid Bech32 address")?;
        Ok(data)
    } else {
        hex::decode(addr).context("Invalid hex address")
    }
}

/// Response from MuesliSwap pool API
#[derive(Debug, Deserialize)]
struct MuesliPoolResponse {
    #[serde(rename = "poolId")]
    pool_id: String,
    #[serde(rename = "tokenA")]
    token_a: MuesliToken,
    #[serde(rename = "tokenB")]
    token_b: MuesliToken,
    #[serde(rename = "reserveA")]
    reserve_a: Option<String>,
    #[serde(rename = "reserveB")]
    reserve_b: Option<String>,
    #[serde(rename = "lpTokens")]
    lp_tokens: Option<String>,
    #[serde(rename = "feeA")]
    fee_a: Option<String>,
    #[serde(rename = "feeB")]
    fee_b: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MuesliToken {
    #[serde(rename = "policyId")]
    policy_id: Option<String>,
    #[serde(rename = "tokenName")]
    token_name: Option<String>,
    symbol: Option<String>,
}

pub struct MuesliSwapConnector {
    config: DexConfig,
    http_client: reqwest::Client,
    blockfrost_key: String,
    blockfrost_url: String,
    api_url: String,
}

impl MuesliSwapConnector {
    pub fn new(config: DexConfig, bf_config: &BlockfrostConfig) -> Result<Self> {
        let blockfrost_url = bf_config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://cardano-mainnet.blockfrost.io/api/v0".to_string());

        let api_url = config
            .api_url
            .clone()
            .unwrap_or_else(|| "https://api.muesliswap.com".to_string());

        Ok(Self {
            config,
            http_client: reqwest::Client::new(),
            blockfrost_key: bf_config.api_key.clone(),
            blockfrost_url,
            api_url,
        })
    }

    /// Fetch pools using MuesliSwap's native API (faster than Blockfrost scanning)
    async fn fetch_pools_via_api(&self) -> Result<Vec<MuesliPoolResponse>> {
        let url = format!("{}/liquidity/pools", self.api_url);

        let resp = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch MuesliSwap pools via API")?;

        let pools: Vec<MuesliPoolResponse> = resp.json().await?;
        Ok(pools)
    }

    fn convert_api_pool(&self, api_pool: &MuesliPoolResponse) -> Result<PoolState> {
        let asset_a = match &api_pool.token_a.policy_id {
            Some(pid) if !pid.is_empty() => AssetId {
                policy_id: pid.clone(),
                asset_name: api_pool
                    .token_a
                    .token_name
                    .clone()
                    .unwrap_or_default(),
            },
            _ => AssetId::ada(),
        };

        let asset_b = match &api_pool.token_b.policy_id {
            Some(pid) if !pid.is_empty() => AssetId {
                policy_id: pid.clone(),
                asset_name: api_pool
                    .token_b
                    .token_name
                    .clone()
                    .unwrap_or_default(),
            },
            _ => AssetId::ada(),
        };

        let reserve_a: u64 = api_pool
            .reserve_a
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        let reserve_b: u64 = api_pool
            .reserve_b
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        let lp_supply: u64 = api_pool
            .lp_tokens
            .as_deref()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);

        Ok(PoolState {
            pool_id: api_pool.pool_id.clone(),
            dex_id: DexId::MuesliSwap,
            asset_a,
            asset_b,
            reserve_a,
            reserve_b,
            lp_supply,
            fee: rust_decimal::Decimal::new(3, 3), // 0.3% default
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            utxo_ref: api_pool.pool_id.clone(),
            pool_type: PoolType::ConstantProduct,
        })
    }
}

#[async_trait::async_trait]
impl super::DexConnector for MuesliSwapConnector {
    fn dex_id(&self) -> DexId {
        DexId::MuesliSwap
    }

    fn name(&self) -> &str {
        "MuesliSwap"
    }

    async fn fetch_pools(&self) -> Result<Vec<PoolState>> {
        debug!("Fetching MuesliSwap pools via API...");
        let api_pools = self.fetch_pools_via_api().await?;

        let mut pools = Vec::new();
        for api_pool in &api_pools {
            if let Ok(pool) = self.convert_api_pool(api_pool) {
                pools.push(pool);
            }
        }

        debug!("Found {} valid MuesliSwap pools", pools.len());
        Ok(pools)
    }

    async fn fetch_pool(&self, pool_id: &str) -> Result<PoolState> {
        let api_pools = self.fetch_pools_via_api().await?;
        api_pools
            .iter()
            .find(|p| p.pool_id == pool_id)
            .map(|p| self.convert_api_pool(p))
            .unwrap_or_else(|| anyhow::bail!("Pool not found: {}", pool_id))?
    }

    fn build_swap_order(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
        min_output: u64,
        receiver_address: &str,
    ) -> Result<SwapOrder> {
        // Build MuesliSwap swap order datum
        let receiver_raw = decode_bech32_to_raw(receiver_address)?;

        // Determine the desired output asset
        let (desired_policy, desired_name) =
            if input_asset == "lovelace" || input_asset == pool.asset_a.to_subject() {
                (pool.asset_b.policy_id.as_str(), pool.asset_b.asset_name.as_str())
            } else {
                // Swapping token for ADA — desired is ADA (empty policy)
                ("", "")
            };

        let datum_data = crate::datum::muesliswap::build_swap_order_datum(
            &receiver_raw,   // sender
            &receiver_raw,   // receiver
            desired_policy,
            desired_name,
            min_output,
            crate::datum::muesliswap::BATCHER_FEE_LOVELACE,
            crate::datum::muesliswap::OUTPUT_ADA_LOVELACE,
        )?;
        let datum = datum_data.to_cbor()?;

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
            dex_id: DexId::MuesliSwap,
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
