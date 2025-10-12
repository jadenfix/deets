use borsh::{BorshDeserialize, BorshSerialize};

use crate::error::{CodecError, Result};

/// Serialize a value using Borsh.
pub fn encode_borsh<T: BorshSerialize>(value: &T) -> Result<Vec<u8>> {
    value.try_to_vec().map_err(CodecError::from)
}

/// Deserialize a value encoded with Borsh.
pub fn decode_borsh<T: BorshDeserialize>(bytes: &[u8]) -> Result<T> {
    T::try_from_slice(bytes).map_err(CodecError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use borsh::{BorshDeserialize, BorshSerialize};

    #[derive(Debug, PartialEq, BorshSerialize, BorshDeserialize)]
    struct Sample {
        id: u32,
        name: String,
    }

    #[test]
    fn roundtrip() {
        let sample = Sample {
            id: 42,
            name: "test".to_string(),
        };

        let encoded = encode_borsh(&sample).unwrap();
        let decoded = decode_borsh::<Sample>(&encoded).unwrap();

        assert_eq!(sample, decoded);
    }

    #[test]
    fn decode_error() {
        let bytes = vec![1, 2, 3];
        let err = decode_borsh::<Sample>(&bytes).unwrap_err();
        assert!(matches!(err, CodecError::Borsh(_)));
    }
}
