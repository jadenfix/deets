use anyhow::Result;

/// Placeholder compression that currently passes data through without modification.
/// This keeps the interface ready for real compression without adding heavy deps.
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(bytes.to_vec())
}

pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    Ok(bytes.to_vec())
}
