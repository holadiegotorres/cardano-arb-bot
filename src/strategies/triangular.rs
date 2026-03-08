//! Triangular Arbitrage Strategy
//!
//! Finds profitable cycles through 3+ tokens/pools.
//! Example: ADA → USDCx → DJED → ADA
//!   - If the cross-rates between stablecoins are slightly off,
//!     cycling through them and back to ADA yields a profit.
//!
//! Uses petgraph to model the token graph and find profitable cycles.

use petgraph::graph::{DiGraph, NodeIndex};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{debug, trace};

use crate::config::StrategiesConfig;
use crate::dex::types::*;
use crate::dex::DexRegistry;
use crate::price_engine::PriceEngine;

/// Edge in the token graph: represents a swap possibility
#[derive(Debug, Clone)]
struct SwapEdge {
    dex_id: DexId,
    pool_id: String,
    pool_state: PoolState,
    /// Effective exchange rate (output/input after fees)
    rate: f64,
}

/// Find triangular arbitrage opportunities using graph-based path analysis
pub fn find_triangular_opportunities(
    engine: &PriceEngine,
    dex_registry: &DexRegistry,
    pools: &[PoolState],
    config: &StrategiesConfig,
) -> Vec<ArbOpportunity> {
    // Build a directed graph where:
    //   - Nodes = token identifiers
    //   - Edges = swap opportunities with exchange rates
    let mut graph = DiGraph::<String, SwapEdge>::new();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    // Helper: get or create a node for a token
    let mut get_node = |graph: &mut DiGraph<String, SwapEdge>,
                        map: &mut HashMap<String, NodeIndex>,
                        asset: &AssetId|
     -> NodeIndex {
        let key = asset.to_subject();
        *map.entry(key.clone())
            .or_insert_with(|| graph.add_node(key))
    };

    // Populate graph from pool states
    for pool in pools {
        if pool.reserve_a == 0 || pool.reserve_b == 0 {
            continue;
        }

        let node_a = get_node(&mut graph, &mut node_map, &pool.asset_a);
        let node_b = get_node(&mut graph, &mut node_map, &pool.asset_b);

        let fee_factor = 1.0 - pool.fee.to_string().parse::<f64>().unwrap_or(0.003);

        // Edge A→B: swap asset_a for asset_b
        let rate_a_to_b = (pool.reserve_b as f64 / pool.reserve_a as f64) * fee_factor;
        graph.add_edge(
            node_a,
            node_b,
            SwapEdge {
                dex_id: pool.dex_id,
                pool_id: pool.pool_id.clone(),
                pool_state: pool.clone(),
                rate: rate_a_to_b,
            },
        );

        // Edge B→A: swap asset_b for asset_a
        let rate_b_to_a = (pool.reserve_a as f64 / pool.reserve_b as f64) * fee_factor;
        graph.add_edge(
            node_b,
            node_a,
            SwapEdge {
                dex_id: pool.dex_id,
                pool_id: pool.pool_id.clone(),
                pool_state: pool.clone(),
                rate: rate_b_to_a,
            },
        );
    }

    let max_hops = config.max_triangular_hops;
    let mut opportunities = Vec::new();

    // Start from ADA (lovelace) — we want to end up with more ADA than we started
    let ada_key = "lovelace".to_string();
    let ada_node = match node_map.get(&ada_key) {
        Some(n) => *n,
        None => return opportunities, // No ADA pools found
    };

    // DFS to find profitable cycles starting and ending at ADA
    let mut path: Vec<(NodeIndex, SwapEdge)> = Vec::new();
    let mut visited: HashMap<NodeIndex, bool> = HashMap::new();

    find_cycles(
        &graph,
        &node_map,
        ada_node,
        ada_node,
        &mut path,
        &mut visited,
        max_hops,
        1.0, // cumulative rate starts at 1.0
        config,
        dex_registry,
        &mut opportunities,
    );

    opportunities
}

/// Recursive DFS to find profitable cycles
fn find_cycles(
    graph: &DiGraph<String, SwapEdge>,
    node_map: &HashMap<String, NodeIndex>,
    current: NodeIndex,
    start: NodeIndex,
    path: &mut Vec<(NodeIndex, SwapEdge)>,
    visited: &mut HashMap<NodeIndex, bool>,
    max_depth: usize,
    cumulative_rate: f64,
    config: &StrategiesConfig,
    dex_registry: &DexRegistry,
    opportunities: &mut Vec<ArbOpportunity>,
) {
    if path.len() >= max_depth {
        return;
    }

    visited.insert(current, true);

    for edge_ref in graph.edges(current) {
        let target = edge_ref.target();
        let edge = edge_ref.weight();
        let new_rate = cumulative_rate * edge.rate;

        // If we've come back to start with a profitable rate
        if target == start && path.len() >= 2 {
            // Account for transaction fees (per hop)
            let num_hops = path.len() + 1;
            let tx_fee_factor = 1.0 - (num_hops as f64 * 0.002); // ~0.2% per hop for tx fees

            let final_rate = new_rate * tx_fee_factor;

            if final_rate > 1.0 {
                // We found a profitable cycle!
                let profit_pct = (final_rate - 1.0) * 100.0;

                // Build the opportunity
                let test_input = 100_000_000u64; // 100 ADA test amount
                let expected_output = (test_input as f64 * final_rate) as u64;
                let profit_lovelace = expected_output.saturating_sub(test_input);
                let profit_ada = Decimal::from(profit_lovelace) / Decimal::from(1_000_000u64);

                let safe_profit = profit_ada
                    * Decimal::try_from(config.profit_safety_factor)
                        .unwrap_or(Decimal::ONE);

                if safe_profit >= config.min_profit_ada {
                    let mut steps = Vec::new();
                    let mut route_parts = Vec::new();

                    // Build steps from path
                    for (i, (node, step_edge)) in path.iter().enumerate() {
                        let input_asset_key = &graph[*node];
                        let output_node = if i + 1 < path.len() {
                            path[i + 1].0
                        } else {
                            target // Last step goes to target (which loops back to start)
                        };
                        let output_asset_key = &graph[output_node];

                        route_parts.push(format!("{}({})", step_edge.dex_id, output_asset_key));

                        steps.push(ArbStep {
                            dex_id: step_edge.dex_id,
                            pool_id: step_edge.pool_id.clone(),
                            input_asset: parse_asset_key(input_asset_key),
                            output_asset: parse_asset_key(output_asset_key),
                            input_amount: 0, // Will be calculated during execution
                            expected_output: 0,
                            min_output: 0,
                        });
                    }

                    // Add final step back to ADA
                    route_parts.push(format!("{}(ADA)", edge.dex_id));
                    steps.push(ArbStep {
                        dex_id: edge.dex_id,
                        pool_id: edge.pool_id.clone(),
                        input_asset: parse_asset_key(&graph[current]),
                        output_asset: AssetId::ada(),
                        input_amount: 0,
                        expected_output: 0,
                        min_output: 0,
                    });

                    let route_desc = format!("ADA → {}", route_parts.join(" → "));

                    let confidence = (profit_pct / 10.0).min(1.0) * 0.7; // Lower confidence for triangular

                    opportunities.push(ArbOpportunity {
                        strategy_type: "Triangular".to_string(),
                        route_description: route_desc,
                        steps,
                        estimated_profit_ada: safe_profit,
                        confidence,
                        input_amount: test_input,
                        input_asset: AssetId::ada(),
                        detected_at_ms: chrono::Utc::now().timestamp_millis() as u64,
                    });

                    trace!(
                        "Triangular cycle found: profit={:.4}% ({} ADA on 100 ADA)",
                        profit_pct,
                        safe_profit
                    );
                }
            }
        }

        // Continue DFS if not visited and not at max depth
        if !visited.get(&target).copied().unwrap_or(false) && path.len() < max_depth - 1 {
            path.push((current, edge.clone()));
            find_cycles(
                graph,
                node_map,
                target,
                start,
                path,
                visited,
                max_depth,
                new_rate,
                config,
                dex_registry,
                opportunities,
            );
            path.pop();
        }
    }

    visited.remove(&current);
}

fn parse_asset_key(key: &str) -> AssetId {
    if key == "lovelace" {
        return AssetId::ada();
    }
    let parts: Vec<&str> = key.split('.').collect();
    if parts.len() == 2 {
        AssetId {
            policy_id: parts[0].to_string(),
            asset_name: parts[1].to_string(),
        }
    } else {
        AssetId {
            policy_id: key.to_string(),
            asset_name: String::new(),
        }
    }
}
