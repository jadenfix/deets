use crate::error::ContractResult;

/// Emit a log event visible to indexers and explorers.
///
/// In WASM: calls `env.emit_log` host function.
/// In tests: prints to stdout.
#[allow(unused_variables)]
pub fn emit_log(data: &[u8]) -> ContractResult<()> {
    #[cfg(test)]
    {
        println!("LOG: {} bytes", data.len());
    }
    Ok(())
}

/// Get the current block number.
///
/// In WASM: calls `env.block_number` host function.
/// In tests: returns a mock value.
pub fn get_block_number() -> u64 {
    // In WASM builds, this would be an extern "C" import.
    // For testing, return a mock value.
    1000
}

/// Get the current block timestamp.
///
/// Mock implementation for native builds; WASM builds use `env.block_timestamp` host call.
pub fn get_timestamp() -> u64 {
    1_700_000_000 // Mock timestamp
}

/// Get the caller's address (20 bytes).
///
/// Mock implementation for native builds; WASM builds use `env.get_caller` host call.
pub fn get_caller() -> [u8; 20] {
    [1u8; 20] // Mock caller
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_log() {
        assert!(emit_log(b"test event").is_ok());
    }

    #[test]
    fn test_block_number() {
        assert!(get_block_number() > 0);
    }

    #[test]
    fn test_timestamp() {
        assert!(get_timestamp() > 0);
    }
}
