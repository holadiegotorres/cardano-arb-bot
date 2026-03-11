//! Cardano DEX Arbitrage Bot
//!
//! A high-performance arbitrage bot for Cardano DEXs, with primary focus on
//! USDCx (the Circle-backed stablecoin launched Feb 2026) and multi-token
//! opportunities across Minswap, SundaeSwap, WingRiders, and MuesliSwap.

mod blockfrost_client;
mod config;
mod datum;
mod dex;
mod executor;
mod price_engine;
mod scanner;
mod strategies;
mod wallet;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use crate::config::BotConfig;
use crate::dex::DexRegistry;
use crate::executor::TransactionExecutor;
use crate::price_engine::PriceEngine;
use crate::scanner::PoolScanner;
use crate::strategies::StrategyEngine;
use crate::wallet::WalletManager;

#[derive(Parser, Debug)]
#[command(name = "cardano-arb-bot")]
#[command(about = "High-performance Cardano DEX arbitrage bot")]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Run in dry-run mode (no real transactions)
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Show detected opportunities without executing
    #[arg(long, default_value_t = false)]
    scan_only: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("=== Cardano ARB Bot v{} ===", env!("CARGO_PKG_VERSION"));
    info!("Loading configuration from: {}", cli.config);

    // Load config
    let config = BotConfig::load(&cli.config)?;

    if cli.dry_run {
        warn!("Running in DRY-RUN mode — no transactions will be submitted");
    }

    // Initialize components
    let dex_registry = Arc::new(DexRegistry::new(&config).await?);
    info!(
        "Initialized {} DEX connectors",
        dex_registry.active_dex_count()
    );

    let wallet = Arc::new(WalletManager::new(&config.wallet)?);
    info!("Wallet loaded: {}", wallet.address_bech32());

    let price_engine = Arc::new(RwLock::new(PriceEngine::new(config.price_engine.clone())));

    let scanner = PoolScanner::new(
        dex_registry.clone(),
        price_engine.clone(),
        config.scanner.clone(),
    );

    let strategy_engine = StrategyEngine::new(
        price_engine.clone(),
        dex_registry.clone(),
        config.strategies.clone(),
    );

    let blockfrost = blockfrost_client::BlockfrostClient::new(&config.blockfrost);

    let executor = TransactionExecutor::new(
        wallet.clone(),
        dex_registry.clone(),
        blockfrost,
        config.executor.clone(),
        cli.dry_run,
    );

    info!("All systems initialized. Starting main loop...");
    info!(
        "Monitoring tokens: {:?}",
        config.scanner.priority_tokens
    );
    info!(
        "Active strategies: DEX-to-DEX={}, Triangular={}",
        config.strategies.enable_dex_to_dex, config.strategies.enable_triangular
    );

    // Main arbitrage loop
    run_bot(scanner, strategy_engine, executor, cli.scan_only).await
}

async fn run_bot(
    mut scanner: PoolScanner,
    strategy_engine: StrategyEngine,
    executor: TransactionExecutor,
    scan_only: bool,
) -> Result<()> {
    let mut scan_interval =
        tokio::time::interval(tokio::time::Duration::from_millis(500));

    loop {
        scan_interval.tick().await;

        // 1. Scan all pools for latest state
        match scanner.scan_all_pools().await {
            Ok(pool_states) => {
                // 2. Feed pool data into strategy engine to find opportunities
                let opportunities = strategy_engine.find_opportunities(&pool_states).await;

                if opportunities.is_empty() {
                    continue;
                }

                for opp in &opportunities {
                    info!(
                        "OPPORTUNITY: {} | profit={} ADA | route={} | confidence={:.2}%",
                        opp.strategy_type, opp.estimated_profit_ada, opp.route_description, opp.confidence * 100.0
                    );
                }

                if scan_only {
                    continue;
                }

                // 3. Execute the best opportunity
                let best = &opportunities[0];
                match executor.execute(best).await {
                    Ok(tx_hash) => {
                        info!("TX SUBMITTED: {} | expected profit: {} ADA", tx_hash, best.estimated_profit_ada);
                    }
                    Err(e) => {
                        error!("Execution failed: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("Scan cycle failed: {}", e);
            }
        }
    }
}
