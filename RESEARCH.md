# Research Notes — Cardano DEX Arbitrage Bot

This document captures all on-chain identifiers, script hashes, datum formats, and documentation references needed to complete the bot implementation.

## USDCx (Circle xReserve USDC Bridge)

| Field | Value |
|-------|-------|
| **Policy ID** | `1f3aec8bfe7ea4fe14c5f121e2a92e301afe414147860d557cac7e34` |
| **Asset Name (hex)** | `5553444378` ("USDCx") |
| **Fingerprint** | `asset1e7eewpjw8ua3f2gpfx7y34ww9vjl63hayn80kl` |
| **Testnet Fingerprint** | `asset1ejelsh8crza8dyghxzsjhkjqutzr7q3dnregng` |
| **Decimals** | 6 |
| **Launch Date** | February 27, 2026 |
| **Backing** | 1:1 by USDC held in Circle xReserve |
| **Supported Apps** | Liqwid, Minswap, SundaeSwap |

Sources:
- https://www.circle.com/blog/usdcx-on-cardano-now-available-via-circle-xreserve
- https://www.thecoinrepublic.com/2026/03/01/cardano-news-cardano-launches-usdc-on-mainnet-over-15-million-tokens-minted/

---

## Minswap V2 (AMM)

**Status: CONFIRMED — All script hashes verified from official GitHub repo**

| Field | Value |
|-------|-------|
| **Pool Script Hash** | `ea07b733d932129c378af627436e7cbc2ef0bf96e0036bb51b3bde6b` |
| **Order Script Hash** | `c3e28c36c3447315ba5a56f33da6a6ddc1770a876a8d9f0cb3a97c4c` |
| **Factory Script Hash** | `7bc5fbd41a95f561be84369631e0e35895efb0b73e0a7480bb9ed730` |
| **Pool NFT Policy ID** | `f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c` |
| **LP Token Policy ID** | `f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c` |
| **Pool Validity Asset** | `f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c.4d5350` |
| **Factory Validity Asset** | `f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c.4d5346` |
| **Pool Creation Address** | `addr1z84q0denmyep98ph3tmzwsmw0j7zau9ljmsqx6a4rvaau66j2c79gy9l76sdg0xwhd7r0c0kna0tycz4y5s6mlenh8pq777e2a` |
| **Batcher Stake Address** | `stake17y02a946720zw6pw50upt2arvxsvvpvaghjtl054h0f0gjsfyjz59` |
| **Cancel Stake Address** | `stake178ytpnrpxax5p8leepgjx9cq8ecedgly6jz4xwvvv4kvzfq9s6295` |
| **Swap Fee** | 0.3% |
| **Batcher Fee** | 2,000,000 lovelace (2 ADA) |
| **Smart Contract Language** | Aiken |

### Minswap V2 Order Datum (SwapExactIn)

The order datum uses Plutus Data Constructor 0 with these fields:

```
Constructor 0 [
    sender: Address,
    receiver: Address,
    receiver_datum_hash: Maybe<Hash>,
    step: SwapExactIn {
        a_to_b_direction: Bool,       // True = swap A→B, False = B→A
        swap_amount_option: Int,       // Amount of input asset to swap
        minimum_receive: Int,          // Minimum output (slippage protection)
        killable: KillOption           // What to do if order can't be filled
    },
    batcher_fee: Int,                  // Fee paid to batcher (lovelace)
    deposit: Int                       // ADA deposit returned after order fills
]
```

Architecture: Batching model — users submit order UTXOs, whitelisted Batchers process them against liquidity pools.

Sources:
- https://github.com/minswap/minswap-dex-v2
- https://github.com/minswap/minswap-dex-v2/blob/main/amm-v2-docs/amm-v2-specs.md
- https://github.com/minswap/sdk (TypeScript SDK with OrderV2 implementation)

### To get exact Plutus Data encoding:
Look at `src/dex-v2/` in the Minswap SDK repo, specifically:
- `order-v2.ts` — contains `OrderV2.toPlutusData()` with exact constructor indices
- `constants.ts` — contains all mainnet constants

---

## SundaeSwap V3

**Status: PARTIAL — Architecture known, exact script hashes need extraction from SDK/source**

| Field | Value |
|-------|-------|
| **SUNDAE Token Policy** | `9a9693a9a37912a5097918f97918d15240c92ab729a0b7c4aa144d77` |
| **Smart Contract Language** | Aiken |
| **Pool Script Hash** | TODO — build from sundae-contracts repo |
| **Order Script Hash** | TODO — check sundae-sdk packages/core |
| **Settings Datum** | Global settings UTXO with authorized scoopers list |

### Validators (from sundae-contracts repo):
- `order.ak` — Users lock funds to signal swap intent
- `pool.ak` — Core CPP-AMM liquidity pool logic
- `settings.ak` — Global protocol settings (authorized scoopers)
- `stake.ak` — Stake withdrawal for "zero withdrawal" trick
- `pool_stake.ak` — Staking validator for LP pools (rewards to treasury)

### Pool Identification:
V3 uses blake2b-256 hash of the first input on the pool creation transaction (with first 4 bytes dropped for CIP-68 label) as the unique pool identifier.

### How to extract script hashes:
1. Clone `github.com/SundaeSwap-finance/sundae-contracts`
2. Build with Aiken to produce the compiled validators
3. Compute the blake2b-224 hash of each compiled script
4. OR: Check `github.com/SundaeSwap-finance/sundae-sdk` → `packages/core/src/` for hardcoded mainnet addresses

Sources:
- https://github.com/SundaeSwap-finance/sundae-contracts
- https://cdn.sundaeswap.finance/SundaeV3.pdf (V3 Whitepaper)
- https://github.com/SundaeSwap-finance/sundae-sdk

---

## WingRiders

**Status: PARTIAL — V1 addresses confirmed, V2 addresses need extraction**

| Field | Value |
|-------|-------|
| **WRT Token Policy** | `c0ee29a85b13209423b10447d3c2e6a50641a15c57770e27cb9d5073` |
| **V1 Pool Address** | `addr1z8nvjzjeydcn4atcd93aac8allvrpjn7pjr2qsweukpnay2lz4g5wy95jwh2l6ca2jyq5xu8aga0fh3jyplef6m0npeslcq0pj` |
| **V1 Request Address** | `addr1wxr2a8htmzuhj39y2gq7ftkpxv98y2g67tg8zezthgq4jkg0a4ul4` |
| **V1 Script Type** | PlutusV1 |
| **V2 Script Type** | Plutarch (PlutusV2) |
| **Swap Fee** | 0.35% |
| **Pool Model** | Each ADA<>Token pool has a unique address |

### Important: Per-Pool Addressing
Unlike Minswap (single pool script address for all pools), WingRiders uses a **different address for each pool**. The `dex-blockfrost-adapter` repo contains a `lpAddressMap` mapping LP token asset names to pool addresses.

### Datum Types (from dex-serializer):
- `LiquidityPoolDatum` — Pool state datum
- Swap request datums (only swap is supported in the serializer lib)
- Uses `@dcspark/cardano-multiplatform-lib` for serialization

### How to get V2 addresses:
1. Check `github.com/WingRiders/dex-blockfrost-adapter` → test/lpmap.mainnet.*.json
2. Check `github.com/WingRiders/dex-serializer` → src/ for datum type definitions
3. Query Blockfrost for known WingRiders pool addresses

Sources:
- https://github.com/WingRiders/dex-blockfrost-adapter
- https://github.com/WingRiders/dex-serializer
- https://medium.com/@wingriderscom/staking-smart-contracts-340a9769aab0

---

## MuesliSwap

**Status: PARTIAL — Architecture known, script hashes need compilation from source**

| Field | Value |
|-------|-------|
| **Smart Contract Base** | Forked from Minswap V1 LP contracts (GPLv3) |
| **Smart Contract Language** | Plutus |
| **Swap Fee** | Parameterized per-pool (not hardcoded) |
| **Batcher Fee** | ~1,700,000 lovelace (1.7 ADA) |
| **API** | `https://api.muesliswap.com` |

### Key Differences from Minswap:
1. LP token minting vulnerability fixed (missing check during pool creation)
2. Protocol fees are parameters stored in pool datum (not hardcoded 0.3%)
3. SwapExactIn/SwapExactOut removed — simplified to constant product formula check
4. Direct matchmaker swaps check that value originates from orderbook script inputs
5. License tokens required for creating pools, batching, and swapping

### How to get script hashes:
1. Clone `github.com/MuesliSwapTeam/muesliswap-cardano-pool-contracts`
2. Build to produce .plutus files
3. Use `cardano-cli address build` to compute script addresses

Sources:
- https://github.com/MuesliSwapTeam/muesliswap-cardano-pool-contracts
- https://github.com/MuesliSwapTeam/muesliswap-cardano-contracts (orderbook)

---

## Other Known Stablecoins

| Token | Policy ID | Asset Name (hex) |
|-------|-----------|------------------|
| **DJED** | `8db269c3ec630e06ae29f74bc39edd1f87c819f1056206e879a1cd61` | `444a4544` |
| **iUSD** | `f66d78b4a3cb3d37afa0ec36461e51ecbde00f26c8f0a68f94b69880` | `69555344` |

## Key Governance Tokens

| Token | Policy ID | Asset Name (hex) |
|-------|-----------|------------------|
| **MIN** | `29d222ce763455e3d7a09a665ce554f00ac89d2e99a1a83d267170c6` | `4d494e` |
| **SUNDAE** | `9a9693a9a37912a5097918f97918d15240c92ab729a0b7c4aa144d77` | `53554e444145` |
| **WRT** | `c0ee29a85b13209423b10447d3c2e6a50641a15c57770e27cb9d5073` | `57696e67526964657273` |

---

## Cardano Technical Context

### eUTXO Model Implications for Arb Bots:
- Each UTXO can only be consumed once — prevents double-spending but creates concurrency challenges
- DEXs use a "batching" architecture: users submit orders as UTXOs, batchers combine and execute them
- Transactions are deterministic: the outcome is known before submission
- Multi-step arb (triangular) requires chaining transactions or batching all swaps together

### Transaction Fees:
- Base fee: ~0.17 ADA for simple transactions
- Plutus execution: additional fee based on script complexity (memory + CPU)
- Total typical swap fee: ~0.3-0.5 ADA per transaction
- Plus DEX batcher fee: 1.7-2.5 ADA depending on DEX

### Useful Rust Crates:
- `pallas-primitives` — Cardano data types for all eras
- `pallas-codec` — CBOR serialization (crucial for datum encoding)
- `pallas-crypto` — Blake2b hashing, Ed25519 signing
- `pallas-txbuilder` — Transaction construction
- `pallas-addresses` — Address encoding/decoding (Bech32)
- `whisky` — Higher-level tx builder wrapping pallas (by Sidan Lab)
- `blockfrost` — Blockfrost API Rust client
