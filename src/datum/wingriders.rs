//! WingRiders Order Datum Encoding
//!
//! WingRiders uses a request datum submitted to the request script address.
//! Unlike Minswap, each pool has its own unique address.
//!
//! RequestDatum (from dex-serializer analysis):
//! Constr 0 [
//!     beneficiary: Address,       // where output tokens go
//!     deadline: POSIXTime,        // order expiry (POSIX milliseconds)
//!     action: RequestAction       // what to do
//! ]
//!
//! RequestAction:
//!   Swap  = Constr 0 [direction: SwapDirection, min_wanted: Int]
//!   ... (Add/Remove liquidity etc.)
//!
//! SwapDirection:
//!   AToB = Constr 0 []
//!   BToA = Constr 1 []

use anyhow::Result;
use super::plutus::PlutusData;

/// WingRiders mainnet constants
pub const WRT_TOKEN_POLICY: &str = "c0ee29a85b13209423b10447d3c2e6a50641a15c57770e27cb9d5073";
pub const LP_TOKEN_POLICY: &str = "026a18d04a0c642759bb3d83b12e3344894e5c1c7b2aeb1a2113a570";

/// V1 known addresses (V2 Plutarch addresses share the same request format)
pub const V1_REQUEST_ADDRESS: &str = "addr1wxr2a8htmzuhj39y2gq7ftkpxv98y2g67tg8zezthgq4jkg0a4ul4";

/// Batcher fee is dynamic on WingRiders — this is a reasonable default
pub const DEFAULT_BATCHER_FEE_LOVELACE: u64 = 2_000_000;

/// Build a WingRiders swap request datum
///
/// # Arguments
/// * `beneficiary_addr_bytes` — Address receiving the swapped tokens
/// * `deadline_posix_ms` — Order expiration (POSIX time in milliseconds)
/// * `a_to_b` — true for A→B, false for B→A
/// * `min_wanted` — Minimum output tokens (slippage protection)
pub fn build_swap_request_datum(
    beneficiary_addr_bytes: &[u8],
    deadline_posix_ms: u64,
    a_to_b: bool,
    min_wanted: u64,
) -> Result<PlutusData> {
    let beneficiary = PlutusData::encode_address(beneficiary_addr_bytes)?;
    let deadline = PlutusData::integer(deadline_posix_ms as i128);

    // SwapDirection
    let direction = if a_to_b {
        PlutusData::constr(0, vec![])
    } else {
        PlutusData::constr(1, vec![])
    };

    // RequestAction::Swap = Constr 0 [direction, min_wanted]
    let action = PlutusData::constr(0, vec![
        direction,
        PlutusData::integer(min_wanted as i128),
    ]);

    // RequestDatum = Constr 0 [beneficiary, deadline, action]
    let datum = PlutusData::constr(0, vec![
        beneficiary,
        deadline,
        action,
    ]);

    Ok(datum)
}

/// Get the deadline as POSIX milliseconds (current time + TTL)
pub fn compute_deadline(ttl_seconds: u64) -> u64 {
    let now = chrono::Utc::now().timestamp_millis() as u64;
    now + (ttl_seconds * 1000)
}
