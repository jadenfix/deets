/// Execution context available to smart contracts.
///
/// Provides information about the current block, caller, and contract.
#[derive(Debug, Clone)]
pub struct ContractContext {
    pub caller: [u8; 20],
    pub contract_address: [u8; 20],
    pub block_number: u64,
    pub timestamp: u64,
    pub value: u128,
}

impl ContractContext {
    /// Get the caller's address as a hex string.
    pub fn caller_hex(&self) -> String {
        hex_encode(&self.caller)
    }

    /// Get the contract's address as a hex string.
    pub fn address_hex(&self) -> String {
        hex_encode(&self.contract_address)
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_context() -> impl Strategy<Value = ContractContext> {
        (
            prop::array::uniform20(any::<u8>()),
            prop::array::uniform20(any::<u8>()),
            any::<u64>(),
            any::<u64>(),
            any::<u128>(),
        )
            .prop_map(|(caller, contract_address, block_number, timestamp, value)| {
                ContractContext {
                    caller,
                    contract_address,
                    block_number,
                    timestamp,
                    value,
                }
            })
    }

    proptest! {
        /// caller_hex always returns a 40-char lowercase hex string.
        #[test]
        fn caller_hex_length_and_format(ctx in arb_context()) {
            let hex = ctx.caller_hex();
            prop_assert_eq!(hex.len(), 40);
            prop_assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// address_hex always returns a 40-char lowercase hex string.
        #[test]
        fn address_hex_length_and_format(ctx in arb_context()) {
            let hex = ctx.address_hex();
            prop_assert_eq!(hex.len(), 40);
            prop_assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// hex encoding is injective — different addresses produce different hex.
        #[test]
        fn hex_encoding_injective(
            a in prop::array::uniform20(any::<u8>()),
            b in prop::array::uniform20(any::<u8>()),
        ) {
            prop_assume!(a != b);
            prop_assert_ne!(hex_encode(&a), hex_encode(&b));
        }

        /// hex_encode roundtrips: decoding the hex recovers original bytes.
        #[test]
        fn hex_encode_decode_roundtrip(bytes in prop::array::uniform20(any::<u8>())) {
            let hex = hex_encode(&bytes);
            let decoded: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            prop_assert_eq!(decoded.as_slice(), &bytes[..]);
        }

        /// Clone produces an equal context.
        #[test]
        fn clone_is_equal(ctx in arb_context()) {
            let cloned = ctx.clone();
            prop_assert_eq!(ctx.caller, cloned.caller);
            prop_assert_eq!(ctx.contract_address, cloned.contract_address);
            prop_assert_eq!(ctx.block_number, cloned.block_number);
            prop_assert_eq!(ctx.timestamp, cloned.timestamp);
            prop_assert_eq!(ctx.value, cloned.value);
        }
    }
}
