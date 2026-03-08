//! DEX Registry and Connectors
//!
//! Provides a unified interface for interacting with multiple Cardano DEXs.
//! Each DEX has its own pool structure and swap mechanism, but they all
//! implement the `DexConnector` trait for uniform access.

pub mod minswap;
pub mod sundaeswap;
pub mod wingriders;
pub mod muesliswap;
pub mod types;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{BotConfig, DexConfig};
use types::{DexId, PoolState, SwapOrder};

/// Trait that every DEX connector must implement
#[async_trait::async_trait]
pub trait DexConnector: Send + Sync {
    /// Unique identifier for this DEX
    fn dex_id(&self) -> DexId;

    /// Human-readable name
    fn name(&self) -> &str;

    /// Fetch current state of all tracked pools
    async fn fetch_pools(&self) -> Result<Vec<PoolState>>;

    /// Fetch state of a specific pool by its identifier
    async fn fetch_pool(&self, pool_id: &str) -> Result<PoolState>;

    /// Build a swap order datum for this DEX
    fn build_swap_order(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
        min_output: u64,
        receiver_address: &str,
    ) -> Result<SwapOrder>;

    /// Get the swap fee for this DEX (as a fraction, e.g., 0.003 for 0.3%)
    fn swap_fee(&self) -> f64;

    /// Get the batcher fee in lovelace
    fn batcher_fee_lovelace(&self) -> u64;

    /// Calculate expected output for a given input (accounting for fees and slippage)
    fn calculate_output(
        &self,
        pool: &PoolState,
        input_asset: &str,
        input_amount: u64,
    ) -> Result<u64>;
}

/// Registry holding all active DEX connectors
pub struct DexRegistry {
    connectors: HashMap<DexId, Arc<dyn DexConnector>>,
}

impl DexRegistry {
    pub async fn new(config: &BotConfig) -> Result<Self> {
        let mut connectors: HashMap<DexId, Arc<dyn DexConnector>> = HashMap::new();

        if let Some(ref cfg) = config.dexes.minswap {
            if cfg.enabled {
                let connector = minswap::MinswapConnector::new(
                    cfg.clone(),
                    &config.blockfrost,
                )?;
                connectors.insert(DexId::Minswap, Arc::new(connector));
            }
        }

        if let Some(ref cfg) = config.dexes.sundaeswap {
            if cfg.enabled {
                let connector = sundaeswap::SundaeSwapConnector::new(
                    cfg.clone(),
                    &config.blockfrost,
                )?;
                connectors.insert(DexId::SundaeSwap, Arc::new(connector));
            }
        }

        if let Some(ref cfg) = config.dexes.wingriders {
            if cfg.enabled {
                let connector = wingriders::WingRidersConnector::new(
                    cfg.clone(),
                    &config.blockfrost,
                )?;
                connectors.insert(DexId::WingRiders, Arc::new(connector));
            }
        }

        if let Some(ref cfg) = config.dexes.muesliswap {
            if cfg.enabled {
                let connector = muesliswap::MuesliSwapConnector::new(
                    cfg.clone(),
                    &config.blockfrost,
                )?;
                connectors.insert(DexId::MuesliSwap, Arc::new(connector));
            }
        }

        Ok(Self { connectors })
    }

    pub fn active_dex_count(&self) -> usize {
        self.connectors.len()
    }

    pub fn get(&self, id: &DexId) -> Option<&Arc<dyn DexConnector>> {
        self.connectors.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = (&DexId, &Arc<dyn DexConnector>)> {
        self.connectors.iter()
    }

    pub fn all_connectors(&self) -> impl Iterator<Item = &Arc<dyn DexConnector>> {
        self.connectors.values()
    }
}
