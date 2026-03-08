//! Pool Scanner
//!
//! Continuously polls all connected DEXs for pool state updates.
//! Feeds data into the PriceEngine for opportunity detection.
//! Uses concurrent fetching across DEXs for minimum latency.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn, info};

use crate::config::ScannerConfig;
use crate::dex::types::{AssetId, DexId, PoolState};
use crate::dex::DexRegistry;
use crate::price_engine::PriceEngine;

pub struct PoolScanner {
    dex_registry: Arc<DexRegistry>,
    price_engine: Arc<RwLock<PriceEngine>>,
    config: ScannerConfig,
    /// Cache of known pools: pool_id → PoolState
    pool_cache: HashMap<String, PoolState>,
    /// Priority token set for fast lookups
    priority_tokens: Vec<AssetId>,
    /// Scan cycle counter
    cycle_count: u64,
}

impl PoolScanner {
    pub fn new(
        dex_registry: Arc<DexRegistry>,
        price_engine: Arc<RwLock<PriceEngine>>,
        config: ScannerConfig,
    ) -> Self {
        let priority_tokens: Vec<AssetId> = config
            .priority_tokens
            .iter()
            .map(|t| {
                if t == "lovelace" || t == "ADA" {
                    AssetId::ada()
                } else {
                    let parts: Vec<&str> = t.split('.').collect();
                    if parts.len() == 2 {
                        AssetId {
                            policy_id: parts[0].to_string(),
                            asset_name: parts[1].to_string(),
                        }
                    } else {
                        AssetId {
                            policy_id: t.clone(),
                            asset_name: String::new(),
                        }
                    }
                }
            })
            .collect();

        Self {
            dex_registry,
            price_engine,
            config,
            pool_cache: HashMap::new(),
            priority_tokens,
            cycle_count: 0,
        }
    }

    /// Run one full scan cycle across all DEXs
    pub async fn scan_all_pools(&mut self) -> Result<Vec<PoolState>> {
        self.cycle_count += 1;
        debug!("Scan cycle #{}", self.cycle_count);

        // Fetch pools from all DEXs concurrently
        let mut fetch_futures = Vec::new();
        for connector in self.dex_registry.all_connectors() {
            let connector = connector.clone();
            fetch_futures.push(tokio::spawn(async move {
                let dex_name = connector.name().to_string();
                match connector.fetch_pools().await {
                    Ok(pools) => (dex_name, Ok(pools)),
                    Err(e) => (dex_name, Err(e)),
                }
            }));
        }

        let mut all_pools = Vec::new();
        for future in fetch_futures {
            match future.await {
                Ok((dex_name, Ok(pools))) => {
                    debug!("{}: fetched {} pools", dex_name, pools.len());
                    all_pools.extend(pools);
                }
                Ok((dex_name, Err(e))) => {
                    warn!("{}: fetch failed: {}", dex_name, e);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        // Filter by minimum liquidity
        let min_liq_lovelace = (self.config.min_pool_liquidity_ada
            * rust_decimal::Decimal::from(1_000_000))
        .to_string()
        .parse::<u64>()
        .unwrap_or(0);

        let filtered: Vec<PoolState> = all_pools
            .into_iter()
            .filter(|p| p.tvl_ada() >= min_liq_lovelace)
            .collect();

        // Prioritize pools containing priority tokens
        let mut prioritized: Vec<PoolState> = Vec::new();
        let mut others: Vec<PoolState> = Vec::new();

        for pool in filtered {
            let is_priority = self.priority_tokens.iter().any(|t| {
                pool.asset_a == *t || pool.asset_b == *t
            });

            if is_priority {
                prioritized.push(pool);
            } else {
                others.push(pool);
            }
        }

        // Combine: priority pools first, then others up to max_pools
        prioritized.extend(others);
        prioritized.truncate(self.config.max_pools);

        // Update cache
        for pool in &prioritized {
            self.pool_cache
                .insert(pool.pool_id.clone(), pool.clone());
        }

        // Update price engine
        {
            let mut engine = self.price_engine.write().await;
            engine.update_pools(&prioritized);
        }

        if self.cycle_count % 100 == 0 {
            info!(
                "Scan stats: cycle={}, pools_tracked={}, cache_size={}",
                self.cycle_count,
                prioritized.len(),
                self.pool_cache.len()
            );
        }

        Ok(prioritized)
    }

    /// Get a cached pool state by ID
    pub fn get_cached_pool(&self, pool_id: &str) -> Option<&PoolState> {
        self.pool_cache.get(pool_id)
    }
}
