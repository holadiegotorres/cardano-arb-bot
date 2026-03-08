//! DEX-to-DEX Arbitrage Strategy
//!
//! The simplest arb: find the same token pair priced differently on two DEXs.
//! Buy where it's cheaper, sell where it's more expensive.
//!
//! Example: USDCx is 0.28 ADA on Minswap but 0.29 ADA on WingRiders.
//! Buy on Minswap, sell on WingRiders → pocket the ~3.5% spread minus fees.

use rust_decimal::Decimal;
use tracing::trace;

use crate::config::StrategiesConfig;
use crate::dex::types::*;
use crate::dex::DexRegistry;
use crate::price_engine::PriceEngine;

/// Scan all asset price books for DEX-to-DEX spread opportunities
pub fn find_dex_to_dex_opportunities(
    engine: &PriceEngine,
    dex_registry: &DexRegistry,
    pools: &[PoolState],
    config: &StrategiesConfig,
) -> Vec<ArbOpportunity> {
    let mut opportunities = Vec::new();

    // For each tracked asset, check if there's a meaningful spread
    let spread_opps = engine.get_spread_opportunities(config.min_profit_ada * Decimal::ZERO); // Get all spreads

    for (book, spread) in spread_opps {
        let best_buy = match book.best_buy() {
            Some(q) => q,
            None => continue,
        };
        let best_sell = match book.best_sell() {
            Some(q) => q,
            None => continue,
        };

        // Must be different DEXs
        if best_buy.dex_id == best_sell.dex_id {
            continue;
        }

        // Find corresponding pool states
        let buy_pool = pools.iter().find(|p| p.pool_id == best_buy.pool_id);
        let sell_pool = pools.iter().find(|p| p.pool_id == best_sell.pool_id);

        let (buy_pool, sell_pool) = match (buy_pool, sell_pool) {
            (Some(b), Some(s)) => (b, s),
            _ => continue,
        };

        // Get DEX connectors for fee calculations
        let buy_connector = match dex_registry.get(&best_buy.dex_id) {
            Some(c) => c,
            None => continue,
        };
        let sell_connector = match dex_registry.get(&best_sell.dex_id) {
            Some(c) => c,
            None => continue,
        };

        // Calculate optimal input amount based on pool depth
        // Don't use more than 1% of the smaller pool's reserves (to limit price impact)
        let max_input_by_depth = std::cmp::min(
            buy_pool.reserve_a / 100,  // 1% of buy pool
            sell_pool.reserve_b / 100, // 1% of sell pool (token side)
        );

        // Try multiple input amounts and find the most profitable
        let test_amounts = [
            max_input_by_depth / 10,
            max_input_by_depth / 4,
            max_input_by_depth / 2,
            max_input_by_depth,
        ];

        for &input_ada in &test_amounts {
            if input_ada == 0 {
                continue;
            }

            // Step 1: Buy the token on the cheaper DEX (spend ADA, get token)
            let tokens_received = match buy_connector.calculate_output(buy_pool, "lovelace", input_ada) {
                Ok(out) => out,
                Err(_) => continue,
            };

            if tokens_received == 0 {
                continue;
            }

            // Step 2: Sell the token on the more expensive DEX (spend token, get ADA)
            let ada_back = match sell_connector.calculate_output(
                sell_pool,
                &book.asset.to_subject(),
                tokens_received,
            ) {
                Ok(out) => out,
                Err(_) => continue,
            };

            // Calculate profit (in lovelace)
            let total_fees = buy_connector.batcher_fee_lovelace()
                + sell_connector.batcher_fee_lovelace()
                + 400_000; // ~0.4 ADA for tx fees (two transactions)

            let profit_lovelace = if ada_back > input_ada + total_fees {
                ada_back - input_ada - total_fees
            } else {
                continue; // Not profitable
            };

            let profit_ada =
                Decimal::from(profit_lovelace) / Decimal::from(1_000_000u64);

            // Apply safety factor
            let safe_profit = profit_ada
                * Decimal::try_from(config.profit_safety_factor).unwrap_or(Decimal::ONE);

            if safe_profit < config.min_profit_ada {
                continue;
            }

            // Calculate confidence based on spread stability and pool depth
            let depth_score = (std::cmp::min(buy_pool.tvl_ada(), sell_pool.tvl_ada()) as f64
                / 1_000_000_000.0) // Normalize by 1000 ADA
                .min(1.0);
            let spread_score = (spread.to_string().parse::<f64>().unwrap_or(0.0) / 5.0).min(1.0);
            let confidence = (depth_score * 0.4 + spread_score * 0.6).min(1.0);

            // Apply slippage tolerance
            let slippage_factor = Decimal::ONE - config.max_slippage_pct / Decimal::from(100);
            let min_ada_back = (Decimal::from(ada_back) * slippage_factor)
                .to_string()
                .parse::<u64>()
                .unwrap_or(0);

            let opp = ArbOpportunity {
                strategy_type: "DEX-to-DEX".to_string(),
                route_description: format!(
                    "Buy {} on {} → Sell on {} (spread: {:.2}%)",
                    book.asset, best_buy.dex_id, best_sell.dex_id, spread
                ),
                steps: vec![
                    ArbStep {
                        dex_id: best_buy.dex_id,
                        pool_id: buy_pool.pool_id.clone(),
                        input_asset: AssetId::ada(),
                        output_asset: book.asset.clone(),
                        input_amount: input_ada,
                        expected_output: tokens_received,
                        min_output: (tokens_received as f64 * 0.98) as u64, // 2% slippage buffer
                    },
                    ArbStep {
                        dex_id: best_sell.dex_id,
                        pool_id: sell_pool.pool_id.clone(),
                        input_asset: book.asset.clone(),
                        output_asset: AssetId::ada(),
                        input_amount: tokens_received,
                        expected_output: ada_back,
                        min_output: min_ada_back,
                    },
                ],
                estimated_profit_ada: safe_profit,
                confidence,
                input_amount: input_ada,
                input_asset: AssetId::ada(),
                detected_at_ms: chrono::Utc::now().timestamp_millis() as u64,
            };

            trace!(
                "DEX-to-DEX: {} profit={} ADA input={} lovelace",
                opp.route_description,
                safe_profit,
                input_ada
            );

            opportunities.push(opp);
        }
    }

    // De-duplicate: keep only the best opportunity per asset
    let mut best_per_asset: std::collections::HashMap<String, ArbOpportunity> =
        std::collections::HashMap::new();

    for opp in opportunities {
        let key = opp.steps[0].output_asset.to_subject();
        let is_better = best_per_asset
            .get(&key)
            .map_or(true, |existing| {
                opp.estimated_profit_ada > existing.estimated_profit_ada
            });
        if is_better {
            best_per_asset.insert(key, opp);
        }
    }

    best_per_asset.into_values().collect()
}
