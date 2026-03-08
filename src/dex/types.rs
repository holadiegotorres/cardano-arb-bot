//! Shared types for DEX interactions.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies which DEX a pool belongs to
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum DexId {
    Minswap,
    SundaeSwap,
    WingRiders,
    MuesliSwap,
    Spectrum,
}

impl fmt::Display for DexId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DexId::Minswap => write!(f, "Minswap"),
            DexId::SundaeSwap => write!(f, "SundaeSwap"),
            DexId::WingRiders => write!(f, "WingRiders"),
            DexId::MuesliSwap => write!(f, "MuesliSwap"),
            DexId::Spectrum => write!(f, "Spectrum"),
        }
    }
}

/// Represents a native asset on Cardano (policy_id + asset_name)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetId {
    /// Hex-encoded policy ID (56 chars / 28 bytes)
    pub policy_id: String,
    /// Hex-encoded asset name
    pub asset_name: String,
}

impl AssetId {
    pub fn ada() -> Self {
        Self {
            policy_id: String::new(),
            asset_name: String::new(),
        }
    }

    pub fn is_ada(&self) -> bool {
        self.policy_id.is_empty()
    }

    /// Returns "policy_id.asset_name" or "ADA"
    pub fn to_subject(&self) -> String {
        if self.is_ada() {
            "lovelace".to_string()
        } else {
            format!("{}.{}", self.policy_id, self.asset_name)
        }
    }

    /// Common known assets
    pub fn usdcx() -> Self {
        // USDCx — Circle's USDC bridge on Cardano via xReserve
        // Launched Feb 27, 2026 on mainnet
        // Fingerprint: asset1e7eewpjw8ua3f2gpfx7y34ww9vjl63hayn80kl
        Self {
            policy_id: "1f3aec8bfe7ea4fe14c5f121e2a92e301afe414147860d557cac7e34".to_string(),
            asset_name: hex::encode("USDCx"),
        }
    }

    pub fn djed() -> Self {
        Self {
            policy_id: "8db269c3ec630e06ae29f74bc39edd1f87c819f1056206e879a1cd61".to_string(),
            asset_name: hex::encode("DJED"),
        }
    }

    pub fn iusd() -> Self {
        Self {
            policy_id: "f66d78b4a3cb3d37afa0ec36461e51ecbde00f26c8f0a68f94b69880".to_string(),
            asset_name: hex::encode("iUSD"),
        }
    }

    pub fn min() -> Self {
        Self {
            policy_id: "29d222ce763455e3d7a09a665ce554f00ac89d2e99a1a83d267170c6".to_string(),
            asset_name: hex::encode("MIN"),
        }
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_subject())
    }
}

/// Snapshot of a liquidity pool's current state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolState {
    /// Unique identifier for this pool (UTXO tx_hash#output_index or pool NFT)
    pub pool_id: String,
    /// Which DEX this pool belongs to
    pub dex_id: DexId,
    /// Asset A in the pool (often ADA)
    pub asset_a: AssetId,
    /// Asset B in the pool
    pub asset_b: AssetId,
    /// Reserve of asset A (in smallest unit)
    pub reserve_a: u64,
    /// Reserve of asset B (in smallest unit)
    pub reserve_b: u64,
    /// LP token total supply
    pub lp_supply: u64,
    /// Pool fee (as decimal, e.g., 0.003 for 0.3%)
    pub fee: Decimal,
    /// Timestamp when this state was fetched (epoch millis)
    pub timestamp_ms: u64,
    /// The UTXO holding the pool's funds
    pub utxo_ref: String,
    /// Pool type (constant product, stableswap, etc.)
    pub pool_type: PoolType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolType {
    ConstantProduct,
    StableSwap { amp: u64 },
    Concentrated { tick_lower: i32, tick_upper: i32 },
}

impl PoolState {
    /// Calculate the price of asset_b in terms of asset_a
    /// For a constant-product pool: price = reserve_a / reserve_b
    pub fn price_b_in_a(&self) -> Decimal {
        if self.reserve_b == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.reserve_a) / Decimal::from(self.reserve_b)
    }

    /// Calculate the price of asset_a in terms of asset_b
    pub fn price_a_in_b(&self) -> Decimal {
        if self.reserve_a == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.reserve_b) / Decimal::from(self.reserve_a)
    }

    /// Calculate output amount for a constant-product swap (x * y = k)
    /// input_is_a: true if swapping asset_a for asset_b
    pub fn calc_constant_product_output(&self, input_amount: u64, input_is_a: bool) -> u64 {
        let (reserve_in, reserve_out) = if input_is_a {
            (self.reserve_a, self.reserve_b)
        } else {
            (self.reserve_b, self.reserve_a)
        };

        // Apply fee: effective_input = input * (1 - fee)
        let fee_numerator = (Decimal::ONE - self.fee) * Decimal::from(1000);
        let fee_num = fee_numerator.to_string().parse::<u64>().unwrap_or(997);

        let numerator = input_amount as u128 * reserve_out as u128 * fee_num as u128;
        let denominator =
            reserve_in as u128 * 1000u128 + input_amount as u128 * fee_num as u128;

        if denominator == 0 {
            return 0;
        }

        (numerator / denominator) as u64
    }

    /// Total liquidity in ADA terms (rough estimate)
    pub fn tvl_ada(&self) -> u64 {
        if self.asset_a.is_ada() {
            self.reserve_a * 2 // Both sides roughly equal value
        } else if self.asset_b.is_ada() {
            self.reserve_b * 2
        } else {
            0 // Can't easily estimate without ADA reference
        }
    }
}

/// A swap order to be submitted to a DEX
#[derive(Debug, Clone)]
pub struct SwapOrder {
    /// Target DEX
    pub dex_id: DexId,
    /// Pool to swap in
    pub pool_id: String,
    /// The CBOR-encoded datum for the swap order
    pub datum: Vec<u8>,
    /// The script address to send the order to
    pub order_address: String,
    /// Value to attach (input token + min ADA + batcher fee)
    pub value_lovelace: u64,
    /// Non-ADA assets to attach
    pub value_assets: Vec<(AssetId, u64)>,
    /// Minimum expected output (for slippage protection)
    pub min_output: u64,
}

/// Represents an arbitrage opportunity found by the strategy engine
#[derive(Debug, Clone)]
pub struct ArbOpportunity {
    /// Type of strategy that found this
    pub strategy_type: String,
    /// Human-readable route description (e.g., "Minswap→WingRiders ADA/USDCx")
    pub route_description: String,
    /// Ordered list of swap steps
    pub steps: Vec<ArbStep>,
    /// Estimated profit in ADA (after all fees)
    pub estimated_profit_ada: Decimal,
    /// Confidence score (0.0-1.0)
    pub confidence: f64,
    /// Input amount in the starting asset
    pub input_amount: u64,
    /// The starting asset
    pub input_asset: AssetId,
    /// Timestamp when opportunity was detected
    pub detected_at_ms: u64,
}

/// A single step in an arbitrage route
#[derive(Debug, Clone)]
pub struct ArbStep {
    pub dex_id: DexId,
    pub pool_id: String,
    pub input_asset: AssetId,
    pub output_asset: AssetId,
    pub input_amount: u64,
    pub expected_output: u64,
    pub min_output: u64,
}
