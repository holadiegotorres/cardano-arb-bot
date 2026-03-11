//! MuesliSwap Order Datum Encoding
//!
//! MuesliSwap forked from Minswap V1. The order datum is similar but simplified:
//! no SwapExactIn/SwapExactOut distinction — uses a direct constant product check.
//!
//! MuesliSwap OrderDatum (from muesliswap-cardano-pool-contracts):
//! Constr 0 [
//!     sender: Address,             // who placed the order (for cancellation)
//!     receiver: Address,           // where output tokens go
//!     receiver_datum_hash: Maybe<Hash>,
//!     step: OrderStep,             // swap details
//!     batcher_fee: Int,            // fee for the batcher
//!     output_ada: Int              // min ADA in output UTXO
//! ]
//!
//! OrderStep for swap:
//!   Swap = Constr 0 [desired_coin: AssetClass, min_receive: Int]
//!
//! AssetClass = Constr 0 [policy_id: ByteString, token_name: ByteString]

use anyhow::Result;
use super::plutus::PlutusData;

/// MuesliSwap mainnet constants
pub const BATCHER_FEE_LOVELACE: u64 = 1_700_000;
pub const OUTPUT_ADA_LOVELACE: u64 = 2_000_000;

/// Build a MuesliSwap swap order datum
///
/// # Arguments
/// * `sender_addr_bytes` — Raw address bytes (for refund on cancel)
/// * `receiver_addr_bytes` — Raw address bytes (where output goes)
/// * `desired_policy_id` — Policy ID of the token you want to receive (hex)
/// * `desired_asset_name` — Asset name of the token you want to receive (hex)
/// * `min_receive` — Minimum tokens to receive (slippage protection)
/// * `batcher_fee` — Batcher fee in lovelace
/// * `output_ada` — Min ADA in the filled order output
pub fn build_swap_order_datum(
    sender_addr_bytes: &[u8],
    receiver_addr_bytes: &[u8],
    desired_policy_id: &str,
    desired_asset_name: &str,
    min_receive: u64,
    batcher_fee: u64,
    output_ada: u64,
) -> Result<PlutusData> {
    let sender = PlutusData::encode_address(sender_addr_bytes)?;
    let receiver = PlutusData::encode_address(receiver_addr_bytes)?;

    // receiver_datum_hash: Nothing = Constr 1 []
    let receiver_datum_hash = PlutusData::constr(1, vec![]);

    // AssetClass = Constr 0 [policy_id, token_name]
    let desired_asset = PlutusData::constr(0, vec![
        PlutusData::bytes(hex::decode(desired_policy_id).unwrap_or_default()),
        PlutusData::bytes(hex::decode(desired_asset_name).unwrap_or_default()),
    ]);

    // OrderStep::Swap = Constr 0 [desired_coin, min_receive]
    let step = PlutusData::constr(0, vec![
        desired_asset,
        PlutusData::integer(min_receive as i128),
    ]);

    // OrderDatum = Constr 0 [sender, receiver, receiver_datum_hash, step, batcher_fee, output_ada]
    let order_datum = PlutusData::constr(0, vec![
        sender,
        receiver,
        receiver_datum_hash,
        step,
        PlutusData::integer(batcher_fee as i128),
        PlutusData::integer(output_ada as i128),
    ]);

    Ok(order_datum)
}
