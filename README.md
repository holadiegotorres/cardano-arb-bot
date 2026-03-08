# Cardano ARB Bot

A high-performance arbitrage bot for Cardano DEXs, built in Rust for maximum execution speed.

## What It Does

Scans liquidity pools across multiple Cardano decentralized exchanges and automatically executes profitable trades when price discrepancies are found.

**Primary focus:** USDCx (Circle's USDC bridge launched Feb 2026) and stablecoin pairs (DJED, iUSD, USDT), but monitors all token pairs with sufficient liquidity.

### Supported DEXs

| DEX | Type | Fee | Notes |
|-----|------|-----|-------|
| Minswap | AMM (Constant Product + StableSwap) | 0.3% | Largest TVL on Cardano |
| SundaeSwap | AMM (Concentrated Liquidity) | 0.3% | First Cardano AMM |
| WingRiders | AMM (Constant Product + StableSwap) | 0.35% | Best stablecoin pools, never hacked |
| MuesliSwap | Hybrid (AMM + Orderbook) | 0.3% | Public REST API |

### Strategies

1. **DEX-to-DEX Arbitrage** — Buy a token where it's cheap, sell where it's expensive
2. **Triangular Arbitrage** — Cycle through 3+ tokens (e.g., ADA → USDCx → DJED → ADA) to exploit cross-rate inefficiencies

## Architecture

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Pool Scanner │────▶│ Price Engine  │────▶│  Strategies  │
│  (per-DEX)   │     │ (EWMA, books)│     │ (d2d + tri)  │
└──────────────┘     └──────────────┘     └──────┬───────┘
                                                  │
                                                  ▼
                     ┌──────────────┐     ┌──────────────┐
                     │    Wallet    │◀────│   Executor   │
                     │  (keys, sig) │     │ (build + tx) │
                     └──────────────┘     └──────────────┘
```

**Scanner** → Fetches pool states from all DEXs via Blockfrost API (concurrent)
**Price Engine** → Maintains cross-DEX price books with EWMA smoothing
**Strategies** → Graph-based path finding (petgraph) for triangular arb
**Executor** → Builds and signs transactions using pallas/whisky, submits via Blockfrost
**Wallet** → Ed25519 key management, UTXO tracking

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- [GitHub CLI](https://cli.github.com/) (for repo setup)
- [Blockfrost API key](https://blockfrost.io/) (free tier works for testing)
- A Cardano wallet with some ADA

### Setup

```bash
# 1. Clone and enter the repo
git clone https://github.com/YOUR_USERNAME/cardano-arb-bot.git
cd cardano-arb-bot

# 2. Copy and edit config
cp config.example.toml config.toml
# Edit config.toml with your Blockfrost key, wallet path, and DEX script hashes

# 3. Build
cargo build --release

# 4. Test (scan-only, no real trades)
./target/release/cardano-arb-bot --dry-run --scan-only

# 5. Test (dry-run with execution logic but no real tx submission)
./target/release/cardano-arb-bot --dry-run

# 6. Go live (CAUTION: real trades with real money)
./target/release/cardano-arb-bot
```

### CLI Options

```
--config <path>     Config file path (default: config.toml)
--dry-run           Don't submit real transactions
--scan-only         Only show opportunities, don't execute
--log-level <level> trace, debug, info, warn, error (default: info)
```

## Configuration

See `config.example.toml` for a fully documented configuration template. Key sections:

- **network** — Mainnet/testnet selection
- **wallet** — Signing key, trade limits, reserves
- **blockfrost** — API credentials and rate limiting
- **scanner** — Poll interval, liquidity filters, priority tokens
- **price_engine** — EWMA smoothing, staleness thresholds
- **strategies** — Enable/disable strategies, profit thresholds, slippage tolerance
- **executor** — TTL, fees, retries
- **dexes** — Per-DEX script hashes and fee configs

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `pallas-*` | Cardano primitives, CBOR, crypto, tx building |
| `whisky` | High-level transaction builder (wraps pallas) |
| `blockfrost` | Blockfrost API client for chain queries |
| `petgraph` | Graph algorithms for triangular arb path finding |
| `rust_decimal` | Precise decimal math (no floating point errors) |
| `tokio` | Async runtime for concurrent DEX polling |
| `reqwest` | HTTP client for API calls |

## Development Status

This is an active project. The following TODO items need completion before going live:

- [ ] Implement proper Plutus datum encoding for each DEX's swap orders
- [ ] Complete transaction building with pallas-txbuilder (UTXO selection, fee calc)
- [ ] Add proper Ed25519 signing with pallas-crypto
- [ ] Fill in actual mainnet script hashes for each DEX
- [ ] Get the real USDCx policy ID
- [ ] Add StableSwap math for WingRiders stablecoin pools
- [ ] Implement UTXO tracking to prevent double-spending in multi-step arb
- [ ] Add Prometheus metrics endpoint
- [ ] Add notification support (email/webhook on profitable trades)
- [ ] Integration tests against preprod testnet

## Security

- **NEVER** commit `config.toml` or wallet key files
- The `.gitignore` is pre-configured to exclude sensitive files
- Start with `--dry-run` and `--scan-only` before risking real funds
- Set conservative `min_profit_ada` and `profit_safety_factor` initially
- Use the `min_ada_reserve` setting to protect against draining your wallet

## License

MIT
