use serde::{de::DeserializeOwned, Serialize};

use crate::error::{CodecError, Result};

/// Serialize a value using bincode with workspace defaults.
pub fn encode_bincode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(CodecError::from)
}

/// Deserialize a value encoded with bincode.
pub fn decode_bincode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    bincode::deserialize(bytes).map_err(CodecError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        id: u64,
        payload: Vec<u8>,
    }

    #[test]
    fn roundtrip() {
        let sample = Sample {
            id: 7,
            payload: vec![1, 2, 3],
        };

        let encoded = encode_bincode(&sample).unwrap();
        let decoded = decode_bincode::<Sample>(&encoded).unwrap();

        assert_eq!(sample, decoded);
    }

    #[test]
    fn decode_error() {
        let bytes = vec![1, 2, 3];
        let err = decode_bincode::<Sample>(&bytes).unwrap_err();
        assert!(matches!(err, CodecError::Bincode(_)));
    }
}
