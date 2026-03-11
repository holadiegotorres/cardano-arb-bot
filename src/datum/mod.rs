//! Plutus Datum Encoding Module
//!
//! Each Cardano DEX expects swap orders in a specific Plutus Data format,
//! encoded as CBOR. This module handles constructing and serializing
//! these datums for each supported DEX.
//!
//! Plutus Data has these constructors:
//!   - Constr(tag, fields)   — tagged constructor with a list of fields
//!   - Map(entries)          — key-value map
//!   - List(items)           — list of data items
//!   - Integer(n)            — arbitrary-precision integer
//!   - ByteString(bytes)     — raw bytes
//!
//! CBOR encoding of Plutus Data uses specific tags:
//!   - Constr 0-6:   tag 121-127
//!   - Constr 7+:    tag 102, [constructor_index, fields]
//!   - Integer:      standard CBOR integer
//!   - ByteString:   CBOR byte string
//!   - List:         CBOR array
//!   - Map:          CBOR map

pub mod minswap;
pub mod sundaeswap;
pub mod wingriders;
pub mod muesliswap;
pub mod plutus;

pub use plutus::PlutusData;
