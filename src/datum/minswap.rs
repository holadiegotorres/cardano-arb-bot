//! Minswap V2 Order Datum Encoding
//!
//! Minswap V2 order datum (from amm-v2-specs.md):
//!
//! OrderDatum = Constr 0 [
//!     sender: Address,
//!     receiver: Address,
//!     receiver_datum_hash: Maybe<Hash>,
//!     step: OrderStep,
//!     batcher_fee: Int,
//!     output_ada: Int                   // min ADA in output UTXO
//! ]
//!
//! OrderStep variants:
//!   SwapExactIn  = Constr 0 [direction: Bool, min_receive: Int]
//!   SwapExactOut = Constr 1 [direction: Bool, expected_receive: Int]
//!   Deposit      = Constr 2 [min_lp: Int]
//!   Withdraw     = Constr 3 [min_a: Int, min_b: Int]
//!   ... (others for zap, etc.)
//!
//! Direction: True (Constr 1 []) = A→B, False (Constr 0 []) = B→A

use anyhow::Result;
use super::plutus::PlutusData;

/// Minswap V2 mainnet constants
pub const POOL_SCRIPT_HASH: &str = "ea07b733d932129c378af627436e7cbc2ef0bf96e0036bb51b3bde6b";
pub const ORDER_SCRIPT_HASH: &str = "c3e28c36c3447315ba5a56f33da6a6ddc1770a876a8d9f0cb3a97c4c";
pub const POOL_NFT_POLICY: &str = "f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c";
pub const LP_TOKEN_POLICY: &str = "f5808c2c990d86da54bfc97d89cee6efa20cd8461616359478d96b4c";
pub const FACTORY_SCRIPT_HASH: &str = "7bc5fbd41a95f561be84369631e0e35895efb0b73e0a7480bb9ed730";
pub const BATCHER_FEE_LOVELACE: u64 = 2_000_000;
pub const OUTPUT_ADA_LOVELACE: u64 = 2_000_000; // Min ADA locked in order UTXO

/// Build a Minswap V2 SwapExactIn order datum
///
/// # Arguments
/// * `sender_addr_bytes` — Raw address bytes of the sender (for refund on cancel)
/// * `receiver_addr_bytes` — Raw address bytes of the receiver (where output goes)
/// * `a_to_b` — true for A→B swap, false for B→A
/// * `min_receive` — Minimum tokens to receive (slippage protection)
/// * `batcher_fee` — Batcher fee in lovelace (typically 2_000_000)
/// * `output_ada` — Min ADA in the filled order output (typically 2_000_000)
pub fn build_swap_exact_in_datum(
    sender_addr_bytes: &[u8],
    receiver_addr_bytes: &[u8],
    a_to_b: bool,
    min_receive: u64,
    batcher_fee: u64,
    output_ada: u64,
) -> Result<PlutusData> {
    let sender = PlutusData::encode_address(sender_addr_bytes)?;
    let receiver = PlutusData::encode_address(receiver_addr_bytes)?;

    // receiver_datum_hash: Nothing = Constr 1 []
    let receiver_datum_hash = PlutusData::constr(1, vec![]);

    // Direction: True = Constr 1 [], False = Constr 0 []
    let direction = if a_to_b {
        PlutusData::constr(1, vec![])
    } else {
        PlutusData::constr(0, vec![])
    };

    // SwapExactIn = Constr 0 [direction, min_receive]
    let step = PlutusData::constr(0, vec![
        direction,
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

/// Build a Minswap V2 SwapExactOut order datum
pub fn build_swap_exact_out_datum(
    sender_addr_bytes: &[u8],
    receiver_addr_bytes: &[u8],
    a_to_b: bool,
    expected_receive: u64,
    batcher_fee: u64,
    output_ada: u64,
) -> Result<PlutusData> {
    let sender = PlutusData::encode_address(sender_addr_bytes)?;
    let receiver = PlutusData::encode_address(receiver_addr_bytes)?;
    let receiver_datum_hash = PlutusData::constr(1, vec![]);

    let direction = if a_to_b {
        PlutusData::constr(1, vec![])
    } else {
        PlutusData::constr(0, vec![])
    };

    // SwapExactOut = Constr 1 [direction, expected_receive]
    let step = PlutusData::constr(1, vec![
        direction,
        PlutusData::integer(expected_receive as i128),
    ]);

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

/// Compute the order address on mainnet:
/// Payment credential = ORDER_SCRIPT_HASH
/// Staking credential = batcher stake key
pub fn order_address_mainnet() -> String {
    // addr1z... with payment=ORDER_SCRIPT_HASH and stake=batcher
    // In practice, use pallas-addresses to construct this
    format!(
        "addr1_order_{}",
        &ORDER_SCRIPT_HASH[..16]
    )
}
