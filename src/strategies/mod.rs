//! Strategy Engine
//!
//! Implements two core arbitrage strategies:
//!   1. DEX-to-DEX: Buy on DEX A, sell on DEX B for the same pair
//!   2. Triangular: A→B→C→A across pools to exploit cross-pair inefficiencies
//!
//! Uses petgraph for efficient path-finding in triangular arbitrage.

pub mod dex_to_dex;
pub mod triangular;

use anyhow::Result;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::StrategiesConfig;
use crate::dex::types::{ArbOpportunity, PoolState};
use crate::dex::DexRegistry;
use crate::price_engine::PriceEngine;

pub struct StrategyEngine {
    price_engine: Arc<RwLock<PriceEngine>>,
    dex_registry: Arc<DexRegistry>,
    config: StrategiesConfig,
}

impl StrategyEngine {
    pub fn new(
        price_engine: Arc<RwLock<PriceEngine>>,
        dex_registry: Arc<DexRegistry>,
        config: StrategiesConfig,
    ) -> Self {
        Self {
            price_engine,
            dex_registry,
            config,
        }
    }

    /// Find all profitable arbitrage opportunities from current pool states
    pub async fn find_opportunities(&self, pools: &[PoolState]) -> Vec<ArbOpportunity> {
        let mut opportunities = Vec::new();

        let engine = self.price_engine.read().await;

        // Strategy 1: DEX-to-DEX
        if self.config.enable_dex_to_dex {
            let dex_opps = dex_to_dex::find_dex_to_dex_opportunities(
                &engine,
                &self.dex_registry,
                pools,
                &self.config,
            );
            debug!("DEX-to-DEX: found {} raw opportunities", dex_opps.len());
            opportunities.extend(dex_opps);
        }

        // Strategy 2: Triangular
        if self.config.enable_triangular {
            let tri_opps = triangular::find_triangular_opportunities(
                &engine,
                &self.dex_registry,
                pools,
                &self.config,
            );
            debug!("Triangular: found {} raw opportunities", tri_opps.len());
            opportunities.extend(tri_opps);
        }

        // Filter by minimum profit and confidence
        opportunities.retain(|opp| {
            opp.estimated_profit_ada >= self.config.min_profit_ada
                && opp.confidence >= self.config.min_confidence
        });

        // Sort by estimated profit (highest first)
        opportunities.sort_by(|a, b| {
            b.estimated_profit_ada
                .partial_cmp(&a.estimated_profit_ada)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if !opportunities.is_empty() {
            info!(
                "Found {} viable opportunities (best: {} ADA profit)",
                opportunities.len(),
                opportunities[0].estimated_profit_ada
            );
        }

        opportunities
    }
}
