//! Core Plutus Data types and CBOR serialization.
//!
//! This implements the Plutus Data encoding used by Cardano smart contracts.
//! All DEX order datums are ultimately serialized to this format.

use anyhow::Result;

/// Represents a Plutus Data value
#[derive(Debug, Clone, PartialEq)]
pub enum PlutusData {
    /// Constructor application: Constr(index, fields)
    /// Index 0-6 maps to CBOR tags 121-127
    /// Index 7+ uses CBOR tag 102 with [index, fields]
    Constr(u64, Vec<PlutusData>),

    /// Arbitrary-precision integer
    Integer(i128),

    /// Raw byte string
    ByteString(Vec<u8>),

    /// Ordered list of data items
    List(Vec<PlutusData>),

    /// Key-value map
    Map(Vec<(PlutusData, PlutusData)>),
}

impl PlutusData {
    /// Encode this PlutusData value to CBOR bytes
    pub fn to_cbor(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.encode_cbor(&mut buf)?;
        Ok(buf)
    }

    /// Encode to a hex string (for debugging / datum hashing)
    pub fn to_cbor_hex(&self) -> Result<String> {
        Ok(hex::encode(self.to_cbor()?))
    }

    fn encode_cbor(&self, buf: &mut Vec<u8>) -> Result<()> {
        match self {
            PlutusData::Constr(index, fields) => {
                if *index <= 6 {
                    // Tags 121-127 for constructors 0-6
                    // CBOR: tag(121 + index) followed by array of fields
                    let tag = 121 + *index;
                    encode_cbor_tag(buf, tag);
                    encode_cbor_array_header(buf, fields.len());
                    for field in fields {
                        field.encode_cbor(buf)?;
                    }
                } else if *index <= 127 {
                    // Tags 1280-1400 for constructors 7-127
                    let tag = 1280 + (*index - 7);
                    encode_cbor_tag(buf, tag);
                    encode_cbor_array_header(buf, fields.len());
                    for field in fields {
                        field.encode_cbor(buf)?;
                    }
                } else {
                    // Tag 102 with [index, fields] for constructor >= 128
                    encode_cbor_tag(buf, 102);
                    encode_cbor_array_header(buf, 2);
                    PlutusData::Integer(*index as i128).encode_cbor(buf)?;
                    encode_cbor_array_header(buf, fields.len());
                    for field in fields {
                        field.encode_cbor(buf)?;
                    }
                }
            }
            PlutusData::Integer(n) => {
                if *n >= 0 {
                    encode_cbor_unsigned(buf, *n as u64);
                } else {
                    // CBOR negative: -1 - n
                    let abs_minus_one = ((-1 - *n) as u64);
                    encode_cbor_negative(buf, abs_minus_one);
                }
            }
            PlutusData::ByteString(bytes) => {
                encode_cbor_bytestring(buf, bytes);
            }
            PlutusData::List(items) => {
                encode_cbor_array_header(buf, items.len());
                for item in items {
                    item.encode_cbor(buf)?;
                }
            }
            PlutusData::Map(entries) => {
                encode_cbor_map_header(buf, entries.len());
                for (key, value) in entries {
                    key.encode_cbor(buf)?;
                    value.encode_cbor(buf)?;
                }
            }
        }
        Ok(())
    }

    // ---- Convenience constructors ----

    pub fn constr(index: u64, fields: Vec<PlutusData>) -> Self {
        PlutusData::Constr(index, fields)
    }

    pub fn integer(n: i128) -> Self {
        PlutusData::Integer(n)
    }

    pub fn bytes(b: Vec<u8>) -> Self {
        PlutusData::ByteString(b)
    }

    pub fn bytes_from_hex(hex_str: &str) -> Result<Self> {
        Ok(PlutusData::ByteString(hex::decode(hex_str)?))
    }

    pub fn list(items: Vec<PlutusData>) -> Self {
        PlutusData::List(items)
    }

    pub fn map(entries: Vec<(PlutusData, PlutusData)>) -> Self {
        PlutusData::Map(entries)
    }

    /// Encode a Cardano address as Plutus Data
    /// Address = Constr 0 [credential, maybe_staking_credential]
    pub fn encode_address(addr_bytes: &[u8]) -> Result<Self> {
        if addr_bytes.is_empty() {
            anyhow::bail!("Empty address bytes");
        }

        let header = addr_bytes[0];
        let addr_type = (header & 0xF0) >> 4;
        let network_id = header & 0x0F;

        // Extract payment credential (bytes 1-28)
        let payment_cred = if addr_bytes.len() >= 29 {
            &addr_bytes[1..29]
        } else {
            anyhow::bail!("Address too short for payment credential");
        };

        // Payment credential: key hash = Constr 0, script hash = Constr 1
        let payment_datum = if addr_type == 0 || addr_type == 2 || addr_type == 4 || addr_type == 6
        {
            // Key hash credential
            PlutusData::constr(0, vec![PlutusData::bytes(payment_cred.to_vec())])
        } else {
            // Script hash credential
            PlutusData::constr(1, vec![PlutusData::bytes(payment_cred.to_vec())])
        };

        // Staking credential (if present, bytes 29-56)
        let staking_datum = if addr_bytes.len() >= 57 {
            let staking_cred = &addr_bytes[29..57];
            let staking_inner = if addr_type == 0 || addr_type == 1 {
                // Staking key hash
                PlutusData::constr(0, vec![PlutusData::bytes(staking_cred.to_vec())])
            } else {
                // Staking script hash
                PlutusData::constr(1, vec![PlutusData::bytes(staking_cred.to_vec())])
            };
            // Some(StakingHash(cred))
            PlutusData::constr(
                0,
                vec![PlutusData::constr(0, vec![staking_inner])],
            )
        } else {
            // Nothing
            PlutusData::constr(1, vec![])
        };

        Ok(PlutusData::constr(0, vec![payment_datum, staking_datum]))
    }
}

// ---- Low-level CBOR encoding helpers ----

fn encode_cbor_tag(buf: &mut Vec<u8>, tag: u64) {
    // Major type 6 (tag)
    encode_cbor_header(buf, 6, tag);
}

fn encode_cbor_unsigned(buf: &mut Vec<u8>, n: u64) {
    // Major type 0 (unsigned integer)
    encode_cbor_header(buf, 0, n);
}

fn encode_cbor_negative(buf: &mut Vec<u8>, n: u64) {
    // Major type 1 (negative integer: -1 - n)
    encode_cbor_header(buf, 1, n);
}

fn encode_cbor_bytestring(buf: &mut Vec<u8>, bytes: &[u8]) {
    // Major type 2 (byte string)
    encode_cbor_header(buf, 2, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn encode_cbor_array_header(buf: &mut Vec<u8>, len: usize) {
    // Major type 4 (array)
    encode_cbor_header(buf, 4, len as u64);
}

fn encode_cbor_map_header(buf: &mut Vec<u8>, len: usize) {
    // Major type 5 (map)
    encode_cbor_header(buf, 5, len as u64);
}

fn encode_cbor_header(buf: &mut Vec<u8>, major_type: u8, value: u64) {
    let mt = major_type << 5;
    if value < 24 {
        buf.push(mt | value as u8);
    } else if value <= 0xFF {
        buf.push(mt | 24);
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(mt | 25);
        buf.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= 0xFFFFFFFF {
        buf.push(mt | 26);
        buf.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        buf.push(mt | 27);
        buf.extend_from_slice(&value.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_integer() {
        let data = PlutusData::integer(42);
        let cbor = data.to_cbor().unwrap();
        assert_eq!(hex::encode(&cbor), "182a"); // CBOR unsigned 42
    }

    #[test]
    fn test_encode_bytestring() {
        let data = PlutusData::bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let cbor = data.to_cbor().unwrap();
        assert_eq!(hex::encode(&cbor), "44deadbeef"); // CBOR bytes(4) + data
    }

    #[test]
    fn test_encode_constr_0() {
        // Constr 0 [] → tag(121) + empty array
        let data = PlutusData::constr(0, vec![]);
        let cbor = data.to_cbor().unwrap();
        assert_eq!(hex::encode(&cbor), "d87980"); // tag(121) + array(0)
    }

    #[test]
    fn test_encode_constr_with_fields() {
        // Constr 0 [Integer(1), Integer(2)]
        let data = PlutusData::constr(0, vec![
            PlutusData::integer(1),
            PlutusData::integer(2),
        ]);
        let cbor = data.to_cbor().unwrap();
        assert_eq!(hex::encode(&cbor), "d8798201 02".replace(" ", ""));
    }

    #[test]
    fn test_encode_list() {
        let data = PlutusData::list(vec![
            PlutusData::integer(1),
            PlutusData::integer(2),
            PlutusData::integer(3),
        ]);
        let cbor = data.to_cbor().unwrap();
        assert_eq!(hex::encode(&cbor), "83010203");
    }
}
