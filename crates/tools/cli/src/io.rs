use std::fs;
use std::io::Write;
use std::path::Path;

use aether_crypto_primitives::Keypair;
use aether_types::{Address, H256};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct KeyFile {
    pub secret_key: String,
    pub public_key: String,
    pub address: String,
}

pub struct KeyMaterial {
    pub keypair: Keypair,
    pub address: Address,
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {}", parent.display()))?;
        }
    }
    Ok(())
}

pub fn write_key_file(path: &Path, keypair: &Keypair) -> Result<()> {
    ensure_parent_dir(path)?;
    let public_key = keypair.public_key();
    let secret_key = keypair.secret_key();
    let address_bytes = keypair.to_address();
    let address = Address::from_slice(&address_bytes)
        .map_err(|e| anyhow::anyhow!("failed to derive address from keypair: {e}"))?;
    let payload = KeyFile {
        secret_key: format!("0x{}", hex::encode(secret_key)),
        public_key: format!("0x{}", hex::encode(public_key)),
        address: format!("0x{}", hex::encode(address.as_bytes())),
    };

    let json = serde_json::to_vec_pretty(&payload)?;
    let mut file = fs::File::create(path)
        .with_context(|| format!("failed to create key file {}", path.display()))?;
    file.write_all(&json)
        .with_context(|| format!("failed to write key file {}", path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush key file {}", path.display()))?;
    Ok(())
}

pub fn read_key_file(path: &Path) -> Result<KeyMaterial> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read key file {}", path.display()))?;
    let parsed: KeyFile = serde_json::from_str(&data)
        .with_context(|| format!("invalid key file {}", path.display()))?;

    let secret_bytes = decode_hex_32(&parsed.secret_key)?;
    let keypair = Keypair::from_bytes(&secret_bytes)
        .map_err(|_| anyhow!("failed to load secret key from {}", path.display()))?;

    // Derive public/address to validate integrity
    let derived_public = keypair.public_key();
    let derived_address_bytes = keypair.to_address();
    let derived_address = Address::from_slice(&derived_address_bytes).map_err(|_| {
        anyhow!(
            "failed to derive address from keypair in {}",
            path.display()
        )
    })?;

    if let Ok(expected_public) = decode_hex_vec(&parsed.public_key) {
        if expected_public != derived_public {
            return Err(anyhow!(
                "public key mismatch in key file {}; re-generate keys",
                path.display()
            ));
        }
    }

    if let Ok(expected_addr) = decode_hex_vec(&parsed.address) {
        if expected_addr != derived_address.as_bytes() {
            return Err(anyhow!(
                "address mismatch in key file {}; re-generate keys",
                path.display()
            ));
        }
    }

    Ok(KeyMaterial {
        keypair,
        address: derived_address,
    })
}

pub fn decode_hex_vec(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.strip_prefix("0x").unwrap_or(input);
    hex::decode(trimmed).map_err(|err| anyhow!("invalid hex string: {err}"))
}

pub fn decode_hex_32(input: &str) -> Result<[u8; 32]> {
    let bytes = decode_hex_vec(input)?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "expected 32-byte secret key, found {} bytes",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

pub fn parse_address(input: &str) -> Result<Address> {
    let bytes = decode_hex_vec(input)?;
    Address::from_slice(&bytes).map_err(|_| {
        anyhow!("address must be 20 bytes (40 hex chars plus 0x prefix), e.g. 0x1234...abcd")
    })
}

pub fn parse_h256(input: &str) -> Result<H256> {
    let bytes = decode_hex_vec(input)?;
    H256::from_slice(&bytes)
        .map_err(|_| anyhow!("hash must be 32 bytes (64 hex chars plus 0x prefix)"))
}

pub fn address_to_string(address: &Address) -> String {
    format!("0x{}", hex::encode(address.as_bytes()))
}

pub fn h256_to_string(value: &H256) -> String {
    format!("0x{}", hex::encode(value.as_bytes()))
}

#[cfg(test)]
mod proptests {
    use super::*;
    use aether_crypto_primitives::Keypair;
    use proptest::prelude::*;
    use tempfile::tempdir;

    // ── hex decode/encode roundtrip ──────────────────────────────────────────

    proptest! {
        /// decode_hex_vec accepts 0x-prefixed and bare hex; encode/decode roundtrip.
        #[test]
        fn prop_decode_hex_vec_roundtrip(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let bare  = hex::encode(&bytes);
            let prefixed = format!("0x{bare}");
            prop_assert_eq!(decode_hex_vec(&bare).unwrap(),     bytes.clone());
            prop_assert_eq!(decode_hex_vec(&prefixed).unwrap(), bytes.clone());
        }

        /// Non-hex input is rejected.
        #[test]
        fn prop_decode_hex_vec_rejects_invalid(
            bad in "[g-z]{1,10}"  // chars outside hex range
        ) {
            prop_assert!(decode_hex_vec(&bad).is_err());
        }

        /// decode_hex_32 succeeds iff input decodes to exactly 32 bytes.
        #[test]
        fn prop_decode_hex_32_length_check(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let s = format!("0x{}", hex::encode(&bytes));
            let result = decode_hex_32(&s);
            if bytes.len() == 32 {
                let arr = result.unwrap();
                prop_assert_eq!(arr.as_ref(), bytes.as_slice());
            } else {
                prop_assert!(result.is_err());
            }
        }
    }

    // ── address parsing ──────────────────────────────────────────────────────

    proptest! {
        /// 20-byte inputs produce a valid Address; any other length is rejected.
        #[test]
        fn prop_parse_address_length(bytes in prop::collection::vec(any::<u8>(), 0..40)) {
            let s = format!("0x{}", hex::encode(&bytes));
            let result = parse_address(&s);
            if bytes.len() == 20 {
                prop_assert!(result.is_ok());
            } else {
                prop_assert!(result.is_err());
            }
        }

        /// address_to_string / parse_address roundtrip.
        #[test]
        fn prop_address_string_roundtrip(raw in prop::array::uniform20(any::<u8>())) {
            let addr = Address::from_slice(&raw).unwrap();
            let s    = address_to_string(&addr);
            let back = parse_address(&s).unwrap();
            prop_assert_eq!(addr, back);
        }
    }

    // ── h256 parsing ─────────────────────────────────────────────────────────

    proptest! {
        /// h256_to_string produces a valid 0x-prefixed 64-char hex string.
        #[test]
        fn prop_h256_string_format(raw in prop::array::uniform32(any::<u8>())) {
            let h   = H256::from_slice(&raw).unwrap();
            let s   = h256_to_string(&h);
            prop_assert!(s.starts_with("0x"));
            prop_assert_eq!(s.len(), 66); // "0x" + 64 hex chars
        }
    }

    // ── key file write / read roundtrip ──────────────────────────────────────

    proptest! {
        /// write_key_file then read_key_file returns the same keypair.
        #[test]
        fn prop_key_file_roundtrip(seed in prop::array::uniform32(any::<u8>())) {
            let dir     = tempdir().unwrap();
            let path    = dir.path().join("test.json");
            let keypair = Keypair::from_bytes(&seed).unwrap();
            write_key_file(&path, &keypair).unwrap();
            let loaded  = read_key_file(&path).unwrap();
            prop_assert_eq!(keypair.public_key(), loaded.keypair.public_key());
            prop_assert_eq!(keypair.to_address(), loaded.keypair.to_address());
        }

        /// Key files from different seeds produce different addresses.
        #[test]
        fn prop_key_file_addresses_differ(
            seed_a in prop::array::uniform32(any::<u8>()),
            seed_b in prop::array::uniform32(any::<u8>())
        ) {
            prop_assume!(seed_a != seed_b);
            let dir = tempdir().unwrap();
            let a   = Keypair::from_bytes(&seed_a).unwrap();
            let b   = Keypair::from_bytes(&seed_b).unwrap();
            prop_assert_ne!(a.to_address(), b.to_address());

            // write/read both to confirm persistence preserves uniqueness
            let pa = dir.path().join("a.json");
            let pb = dir.path().join("b.json");
            write_key_file(&pa, &a).unwrap();
            write_key_file(&pb, &b).unwrap();
            let la = read_key_file(&pa).unwrap();
            let lb = read_key_file(&pb).unwrap();
            prop_assert_ne!(la.address, lb.address);
        }
    }

    // ── config path expansion ────────────────────────────────────────────────

    proptest! {
        /// expand_path on a non-tilde path returns the input as a PathBuf.
        #[test]
        fn prop_expand_path_no_tilde(
            segment in "[a-zA-Z0-9_\\-\\.]{1,20}"
        ) {
            let input = format!("/tmp/{segment}");
            let result = crate::config::expand_path(&input).unwrap();
            prop_assert_eq!(result.to_str().unwrap(), input.as_str());
        }
    }
}
