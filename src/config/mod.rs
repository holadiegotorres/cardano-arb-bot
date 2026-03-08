//! Configuration module — loads and validates bot settings from TOML.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct BotConfig {
    pub network: NetworkConfig,
    pub wallet: WalletConfig,
    pub blockfrost: BlockfrostConfig,
    pub scanner: ScannerConfig,
    pub price_engine: PriceEngineConfig,
    pub strategies: StrategiesConfig,
    pub executor: ExecutorConfig,
    pub dexes: DexesConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    /// "mainnet" or "preprod" or "preview"
    pub network_id: String,
    /// Cardano node socket path (optional, for direct node connection)
    pub node_socket: Option<String>,
    /// Protocol magic number
    pub protocol_magic: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WalletConfig {
    /// Path to signing key file (CBOR hex or Bech32)
    pub signing_key_path: String,
    /// Minimum ADA to keep in wallet (never go below this)
    pub min_ada_reserve: Decimal,
    /// Maximum ADA to risk per trade
    pub max_trade_amount_ada: Decimal,
    /// Collateral UTXO (for Plutus script interactions)
    pub collateral_utxo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockfrostConfig {
    /// Blockfrost API key
    pub api_key: String,
    /// Base URL (defaults to mainnet)
    pub base_url: Option<String>,
    /// Requests per second limit
    pub rate_limit_rps: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScannerConfig {
    /// How often to poll pools (milliseconds)
    pub poll_interval_ms: u64,
    /// Priority tokens to always monitor (policy_id.asset_name hex)
    pub priority_tokens: Vec<String>,
    /// Minimum pool liquidity (in ADA) to consider
    pub min_pool_liquidity_ada: Decimal,
    /// Maximum number of pools to track simultaneously
    pub max_pools: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PriceEngineConfig {
    /// Price staleness threshold (milliseconds) — discard prices older than this
    pub max_price_age_ms: u64,
    /// Minimum price difference to flag (percentage, e.g., 0.5 = 0.5%)
    pub min_spread_pct: Decimal,
    /// Enable EWMA smoothing for price signals
    pub enable_ewma: bool,
    /// EWMA alpha (0.0-1.0, higher = more reactive)
    pub ewma_alpha: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategiesConfig {
    /// Enable direct DEX-to-DEX arbitrage
    pub enable_dex_to_dex: bool,
    /// Enable triangular arbitrage (A→B→C→A)
    pub enable_triangular: bool,
    /// Maximum hops for triangular routes
    pub max_triangular_hops: usize,
    /// Minimum profit threshold in ADA (after fees) to execute
    pub min_profit_ada: Decimal,
    /// Minimum confidence score (0.0-1.0) to execute
    pub min_confidence: f64,
    /// Maximum slippage tolerance (percentage)
    pub max_slippage_pct: Decimal,
    /// Factor of safety on profit estimates (0.0-1.0, lower = more conservative)
    pub profit_safety_factor: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutorConfig {
    /// Transaction TTL in slots (how long tx is valid)
    pub tx_ttl_slots: u64,
    /// Maximum fee willing to pay (in lovelace)
    pub max_fee_lovelace: u64,
    /// Number of confirmation blocks to wait
    pub confirmation_blocks: u32,
    /// Retry count on failed submission
    pub max_retries: u32,
    /// Delay between retries (milliseconds)
    pub retry_delay_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexesConfig {
    pub minswap: Option<DexConfig>,
    pub sundaeswap: Option<DexConfig>,
    pub wingriders: Option<DexConfig>,
    pub muesliswap: Option<DexConfig>,
    pub spectrum: Option<DexConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DexConfig {
    /// Whether this DEX is enabled
    pub enabled: bool,
    /// Script address for the DEX pool validator
    pub pool_script_hash: String,
    /// Order script address (where swap orders are submitted)
    pub order_script_hash: String,
    /// NFT policy ID used to identify legitimate pools
    pub pool_nft_policy_id: String,
    /// Swap fee (percentage, e.g., 0.3 = 0.3%)
    pub swap_fee_pct: Decimal,
    /// Batcher fee in lovelace
    pub batcher_fee_lovelace: u64,
    /// API endpoint for pool data (if available)
    pub api_url: Option<String>,
}

impl BotConfig {
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("Failed to read config file: {}", path))?;

        let config: BotConfig =
            toml::from_str(&content).with_context(|| "Failed to parse config TOML")?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.wallet.min_ada_reserve > Decimal::ZERO,
            "min_ada_reserve must be positive"
        );
        anyhow::ensure!(
            self.wallet.max_trade_amount_ada > Decimal::ZERO,
            "max_trade_amount_ada must be positive"
        );
        anyhow::ensure!(
            self.strategies.min_profit_ada > Decimal::ZERO,
            "min_profit_ada must be positive"
        );
        anyhow::ensure!(
            self.strategies.min_confidence > 0.0 && self.strategies.min_confidence <= 1.0,
            "min_confidence must be between 0.0 and 1.0"
        );
        anyhow::ensure!(
            self.strategies.max_slippage_pct > Decimal::ZERO,
            "max_slippage_pct must be positive"
        );

        // Ensure at least one DEX is enabled
        let any_enabled = [
            self.dexes.minswap.as_ref().map_or(false, |d| d.enabled),
            self.dexes.sundaeswap.as_ref().map_or(false, |d| d.enabled),
            self.dexes.wingriders.as_ref().map_or(false, |d| d.enabled),
            self.dexes.muesliswap.as_ref().map_or(false, |d| d.enabled),
            self.dexes.spectrum.as_ref().map_or(false, |d| d.enabled),
        ]
        .iter()
        .any(|&e| e);

        anyhow::ensure!(any_enabled, "At least one DEX must be enabled");

        Ok(())
    }
}
