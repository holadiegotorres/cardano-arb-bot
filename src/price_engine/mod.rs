//! Price Engine
//!
//! Maintains a real-time price index across all DEX pools.
//! Detects price discrepancies that may indicate arbitrage opportunities.
//! Supports EWMA smoothing to filter noise and reduce false positives.

use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::trace;

use crate::config::PriceEngineConfig;
use crate::dex::types::{AssetId, DexId, PoolState};

/// A price quote from a specific DEX pool
#[derive(Debug, Clone)]
pub struct PriceQuote {
    pub dex_id: DexId,
    pub pool_id: String,
    pub asset: AssetId,
    /// Price in ADA (lovelace per unit of asset)
    pub price_ada: Decimal,
    /// Pool liquidity depth (reserve of the asset)
    pub depth: u64,
    /// Timestamp
    pub timestamp_ms: u64,
}

/// Tracks the best bid/ask across DEXs for a given asset
#[derive(Debug, Clone)]
pub struct AssetPriceBook {
    pub asset: AssetId,
    /// All current quotes, keyed by dex_id
    pub quotes: HashMap<DexId, PriceQuote>,
    /// EWMA-smoothed mid price
    pub ewma_price: Option<f64>,
}

impl AssetPriceBook {
    pub fn new(asset: AssetId) -> Self {
        Self {
            asset,
            quotes: HashMap::new(),
            ewma_price: None,
        }
    }

    /// Get the cheapest place to BUY this asset (lowest ask)
    pub fn best_buy(&self) -> Option<&PriceQuote> {
        self.quotes.values().min_by(|a, b| a.price_ada.cmp(&b.price_ada))
    }

    /// Get the most expensive place to SELL this asset (highest bid)
    pub fn best_sell(&self) -> Option<&PriceQuote> {
        self.quotes.values().max_by(|a, b| a.price_ada.cmp(&b.price_ada))
    }

    /// Calculate the spread between best buy and best sell (as percentage)
    pub fn spread_pct(&self) -> Option<Decimal> {
        let buy = self.best_buy()?;
        let sell = self.best_sell()?;

        if buy.price_ada.is_zero() || buy.dex_id == sell.dex_id {
            return None;
        }

        let spread = (sell.price_ada - buy.price_ada) / buy.price_ada * Decimal::from(100);
        Some(spread)
    }
}

pub struct PriceEngine {
    config: PriceEngineConfig,
    /// Asset → PriceBook
    price_books: HashMap<String, AssetPriceBook>,
}

impl PriceEngine {
    pub fn new(config: PriceEngineConfig) -> Self {
        Self {
            config,
            price_books: HashMap::new(),
        }
    }

    /// Update price data from a batch of pool states
    pub fn update_pools(&mut self, pools: &[PoolState]) {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        for pool in pools {
            // For pools with ADA as one side, we can directly compute prices
            if pool.asset_a.is_ada() && pool.reserve_a > 0 && pool.reserve_b > 0 {
                let price_ada = Decimal::from(pool.reserve_a) / Decimal::from(pool.reserve_b);

                let quote = PriceQuote {
                    dex_id: pool.dex_id,
                    pool_id: pool.pool_id.clone(),
                    asset: pool.asset_b.clone(),
                    price_ada,
                    depth: pool.reserve_b,
                    timestamp_ms: pool.timestamp_ms,
                };

                self.update_quote(quote);
            } else if pool.asset_b.is_ada() && pool.reserve_a > 0 && pool.reserve_b > 0 {
                let price_ada = Decimal::from(pool.reserve_b) / Decimal::from(pool.reserve_a);

                let quote = PriceQuote {
                    dex_id: pool.dex_id,
                    pool_id: pool.pool_id.clone(),
                    asset: pool.asset_a.clone(),
                    price_ada,
                    depth: pool.reserve_a,
                    timestamp_ms: pool.timestamp_ms,
                };

                self.update_quote(quote);
            }
            // For non-ADA pairs, we'd need cross-referencing (triangular)
        }

        // Prune stale quotes
        let max_age = self.config.max_price_age_ms;
        for book in self.price_books.values_mut() {
            book.quotes.retain(|_, q| now_ms - q.timestamp_ms < max_age);
        }
    }

    fn update_quote(&mut self, quote: PriceQuote) {
        let key = quote.asset.to_subject();

        let book = self
            .price_books
            .entry(key)
            .or_insert_with(|| AssetPriceBook::new(quote.asset.clone()));

        // Update EWMA
        if self.config.enable_ewma {
            let price_f64 = quote.price_ada.to_string().parse::<f64>().unwrap_or(0.0);
            let alpha = self.config.ewma_alpha;

            book.ewma_price = Some(match book.ewma_price {
                Some(prev) => alpha * price_f64 + (1.0 - alpha) * prev,
                None => price_f64,
            });
        }

        trace!(
            "Price update: {} on {} = {} ADA",
            quote.asset, quote.dex_id, quote.price_ada
        );

        book.quotes.insert(quote.dex_id, quote);
    }

    /// Get all assets that have a meaningful spread across DEXs
    pub fn get_spread_opportunities(&self, min_spread_pct: Decimal) -> Vec<(&AssetPriceBook, Decimal)> {
        self.price_books
            .values()
            .filter_map(|book| {
                if book.quotes.len() < 2 {
                    return None;
                }
                let spread = book.spread_pct()?;
                if spread >= min_spread_pct {
                    Some((book, spread))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the price book for a specific asset
    pub fn get_price_book(&self, asset_subject: &str) -> Option<&AssetPriceBook> {
        self.price_books.get(asset_subject)
    }

    /// Get all tracked assets
    pub fn tracked_assets(&self) -> Vec<&str> {
        self.price_books.keys().map(|k| k.as_str()).collect()
    }

    /// Get all pools that contain a given asset, across all DEXs
    pub fn pools_for_asset(&self, asset_subject: &str) -> Vec<&PriceQuote> {
        self.price_books
            .get(asset_subject)
            .map(|book| book.quotes.values().collect())
            .unwrap_or_default()
    }
}
