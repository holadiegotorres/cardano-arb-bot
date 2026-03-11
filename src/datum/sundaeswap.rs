//! SundaeSwap V3 Order Datum Encoding
//!
//! SundaeSwap V3 order format (from sundae-contracts/validators/order.ak):
//!
//! Order = Constr 0 [
//!     pool_ident: ByteString,          // blake2b-256 hash identifying the pool
//!     owner: MultiSigScript,           // who can cancel the order
//!     max_protocol_fee: Int,           // max fee willing to pay
//!     destination: Destination,         // where output goes
//!     details: OrderDetails            // what kind of order
//! ]
//!
//! Destination = Constr 0 [address: Address, datum: Maybe<Datum>]
//!
//! OrderDetails variants:
//!   Swap = Constr 0 [offer: (PolicyId, AssetName, Int), min_received: (PolicyId, AssetName, Int)]
//!
//! MultiSigScript:
//!   Signature = Constr 0 [key_hash: ByteString]

use anyhow::Result;
use super::plutus::PlutusData;

/// SundaeSwap V3 mainnet constants
/// NOTE: These need verification — extracted from SDK source code references.
/// The SUNDAE token policy is confirmed: 9a9693a9a37912a5097918f97918d15240c92ab729a0b7c4aa144d77
pub const SUNDAE_TOKEN_POLICY: &str = "9a9693a9a37912a5097918f97918d15240c92ab729a0b7c4aa144d77";
pub const BASE_PROTOCOL_FEE_LOVELACE: u64 = 500_000; // 0.50 ADA base fee
pub const BATCHER_FEE_LOVELACE: u64 = 2_500_000;

/// Build a SundaeSwap V3 swap order datum
///
/// # Arguments
/// * `pool_ident` — Pool identifier (blake2b-256 hash of first input at pool creation)
/// * `owner_key_hash` — Payment key hash of the order creator (for cancellation)
/// * `receiver_addr_bytes` — Where the swapped tokens go
/// * `offer_policy` — Policy ID of offered token ("" for ADA)
/// * `offer_asset_name` — Asset name of offered token ("" for ADA)
/// * `offer_amount` — Amount of offered token
/// * `min_received_policy` — Policy ID of desired token
/// * `min_received_asset_name` — Asset name of desired token
/// * `min_received_amount` — Minimum amount to receive
/// * `max_protocol_fee` — Maximum protocol fee willing to pay (lovelace)
pub fn build_swap_order_datum(
    pool_ident: &[u8],
    owner_key_hash: &[u8],
    receiver_addr_bytes: &[u8],
    offer_policy: &str,
    offer_asset_name: &str,
    offer_amount: u64,
    min_received_policy: &str,
    min_received_asset_name: &str,
    min_received_amount: u64,
    max_protocol_fee: u64,
) -> Result<PlutusData> {
    // pool_ident: ByteString
    let pool_id = PlutusData::bytes(pool_ident.to_vec());

    // owner: Signature(key_hash) = Constr 0 [key_hash]
    let owner = PlutusData::constr(0, vec![
        PlutusData::bytes(owner_key_hash.to_vec()),
    ]);

    // max_protocol_fee: Int
    let max_fee = PlutusData::integer(max_protocol_fee as i128);

    // destination: Constr 0 [address, Nothing]
    let dest_address = PlutusData::encode_address(receiver_addr_bytes)?;
    let destination = PlutusData::constr(0, vec![
        dest_address,
        PlutusData::constr(1, vec![]), // No datum attached to output
    ]);

    // offer: (policy_id, asset_name, amount)
    let offer_triple = PlutusData::constr(0, vec![
        PlutusData::bytes(hex::decode(offer_policy).unwrap_or_default()),
        PlutusData::bytes(hex::decode(offer_asset_name).unwrap_or_default()),
        PlutusData::integer(offer_amount as i128),
    ]);

    // min_received: (policy_id, asset_name, amount)
    let min_received_triple = PlutusData::constr(0, vec![
        PlutusData::bytes(hex::decode(min_received_policy).unwrap_or_default()),
        PlutusData::bytes(hex::decode(min_received_asset_name).unwrap_or_default()),
        PlutusData::integer(min_received_amount as i128),
    ]);

    // details: Swap = Constr 0 [offer, min_received]
    let details = PlutusData::constr(0, vec![
        offer_triple,
        min_received_triple,
    ]);

    // Order = Constr 0 [pool_ident, owner, max_protocol_fee, destination, details]
    let order_datum = PlutusData::constr(0, vec![
        pool_id,
        owner,
        max_fee,
        destination,
        details,
    ]);

    Ok(order_datum)
}
