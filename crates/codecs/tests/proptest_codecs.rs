use borsh::{BorshDeserialize, BorshSerialize};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

use aether_codecs::*;

/// Test struct that implements both Serde and Borsh for cross-format testing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
struct TestRecord {
    id: u64,
    amount: u128,
    label: String,
    data: Vec<u8>,
    flag: bool,
}

fn arb_record() -> impl Strategy<Value = TestRecord> {
    (
        any::<u64>(),
        any::<u128>(),
        "[a-zA-Z0-9]{0,64}",
        proptest::collection::vec(any::<u8>(), 0..256),
        any::<bool>(),
    )
        .prop_map(|(id, amount, label, data, flag)| TestRecord {
            id,
            amount,
            label,
            data,
            flag,
        })
}

proptest! {
    // --- Bincode ---

    #[test]
    fn bincode_roundtrip(record in arb_record()) {
        let encoded = encode_bincode(&record).unwrap();
        let decoded: TestRecord = decode_bincode(&encoded).unwrap();
        prop_assert_eq!(&record, &decoded);
    }

    #[test]
    fn bincode_deterministic(record in arb_record()) {
        let a = encode_bincode(&record).unwrap();
        let b = encode_bincode(&record).unwrap();
        prop_assert_eq!(a, b, "bincode encoding must be deterministic");
    }

    #[test]
    fn bincode_rejects_truncated(record in arb_record()) {
        let encoded = encode_bincode(&record).unwrap();
        if encoded.len() > 1 {
            let truncated = &encoded[..encoded.len() / 2];
            prop_assert!(decode_bincode::<TestRecord>(truncated).is_err());
        }
    }

    #[test]
    fn bincode_rejects_empty(_dummy in 0u8..1) {
        let result = decode_bincode::<TestRecord>(&[]);
        prop_assert!(result.is_err());
    }

    #[test]
    fn bincode_different_inputs_different_bytes(
        a in arb_record(),
        b in arb_record(),
    ) {
        prop_assume!(a != b);
        let ea = encode_bincode(&a).unwrap();
        let eb = encode_bincode(&b).unwrap();
        prop_assert_ne!(ea, eb, "distinct values should produce distinct encodings");
    }

    // --- Borsh ---

    #[test]
    fn borsh_roundtrip(record in arb_record()) {
        let encoded = encode_borsh(&record).unwrap();
        let decoded: TestRecord = decode_borsh(&encoded).unwrap();
        prop_assert_eq!(&record, &decoded);
    }

    #[test]
    fn borsh_deterministic(record in arb_record()) {
        let a = encode_borsh(&record).unwrap();
        let b = encode_borsh(&record).unwrap();
        prop_assert_eq!(a, b, "borsh encoding must be deterministic");
    }

    #[test]
    fn borsh_rejects_truncated(record in arb_record()) {
        let encoded = encode_borsh(&record).unwrap();
        if encoded.len() > 1 {
            let truncated = &encoded[..encoded.len() / 2];
            prop_assert!(decode_borsh::<TestRecord>(truncated).is_err());
        }
    }

    #[test]
    fn borsh_rejects_empty(_dummy in 0u8..1) {
        let result = decode_borsh::<TestRecord>(&[]);
        prop_assert!(result.is_err());
    }

    #[test]
    fn borsh_different_inputs_different_bytes(
        a in arb_record(),
        b in arb_record(),
    ) {
        prop_assume!(a != b);
        let ea = encode_borsh(&a).unwrap();
        let eb = encode_borsh(&b).unwrap();
        prop_assert_ne!(ea, eb, "distinct values should produce distinct encodings");
    }

    // --- Cross-format ---

    #[test]
    fn bincode_borsh_not_interchangeable(record in arb_record()) {
        let bincode_bytes = encode_bincode(&record).unwrap();
        let borsh_bytes = encode_borsh(&record).unwrap();
        // Formats use different layouts; decoding one format with the other
        // should either fail or produce a different value.
        if let Ok(cross) = decode_borsh::<TestRecord>(&bincode_bytes) {
            prop_assert_ne!(&cross, &record,
                "bincode bytes decoded as borsh should not match original");
        }
        if let Ok(cross) = decode_bincode::<TestRecord>(&borsh_bytes) {
            prop_assert_ne!(&cross, &record,
                "borsh bytes decoded as bincode should not match original");
        }
    }

    // --- Primitive types ---

    #[test]
    fn bincode_u128_roundtrip(val in any::<u128>()) {
        let encoded = encode_bincode(&val).unwrap();
        let decoded: u128 = decode_bincode(&encoded).unwrap();
        prop_assert_eq!(val, decoded);
    }

    #[test]
    fn borsh_u128_roundtrip(val in any::<u128>()) {
        let encoded = encode_borsh(&val).unwrap();
        let decoded: u128 = decode_borsh(&encoded).unwrap();
        prop_assert_eq!(val, decoded);
    }

    #[test]
    fn bincode_vec_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let encoded = encode_bincode(&data).unwrap();
        let decoded: Vec<u8> = decode_bincode(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }

    #[test]
    fn borsh_vec_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let encoded = encode_borsh(&data).unwrap();
        let decoded: Vec<u8> = decode_borsh(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }
}
