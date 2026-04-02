use aether_crypto_primitives::Keypair;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;

/// Key file format for secure storage.
/// Keys are stored as JSON with a type discriminator.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyFile {
    pub version: u32,
    pub key_type: KeyType,
    pub public_key_hex: String,
    /// Secret key bytes, hex-encoded. In production this would be encrypted.
    pub secret_key_hex: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeyType {
    Ed25519,
    Vrf,
    Bls,
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyType::Ed25519 => write!(f, "ed25519"),
            KeyType::Vrf => write!(f, "vrf"),
            KeyType::Bls => write!(f, "bls"),
        }
    }
}

/// Generate a new keypair of the given type and save to a file.
pub fn generate_key(key_type: KeyType, output_path: &Path) -> Result<KeyFile> {
    let keypair = Keypair::generate();
    let public_key_hex = hex::encode(keypair.public_key());
    let secret_key_hex = hex::encode(keypair.secret_key());

    let key_file = KeyFile {
        version: 1,
        key_type,
        public_key_hex,
        secret_key_hex,
    };

    let json = serde_json::to_string_pretty(&key_file)?;
    write_key_file_secure(output_path, &json).context("failed to write key file")?;

    Ok(key_file)
}

/// Load a key file from disk.
pub fn load_key(path: &Path) -> Result<KeyFile> {
    let contents = fs::read_to_string(path).context("failed to read key file")?;
    let key_file: KeyFile = serde_json::from_str(&contents).context("failed to parse key file")?;

    if key_file.version != 1 {
        bail!("unsupported key file version: {}", key_file.version);
    }

    Ok(key_file)
}

/// Import a secret key from raw hex and save to a file.
pub fn import_key(key_type: KeyType, secret_hex: &str, output_path: &Path) -> Result<KeyFile> {
    let secret_bytes =
        hex::decode(secret_hex.trim_start_matches("0x")).context("invalid hex for secret key")?;

    if secret_bytes.len() != 32 {
        bail!("secret key must be 32 bytes, got {}", secret_bytes.len());
    }

    let keypair = Keypair::from_bytes(&secret_bytes)
        .map_err(|e| anyhow::anyhow!("invalid secret key: {}", e))?;
    let public_key_hex = hex::encode(keypair.public_key());
    let secret_key_hex = hex::encode(keypair.secret_key());

    let key_file = KeyFile {
        version: 1,
        key_type,
        public_key_hex,
        secret_key_hex,
    };

    let json = serde_json::to_string_pretty(&key_file)?;
    write_key_file_secure(output_path, &json).context("failed to write key file")?;

    Ok(key_file)
}

/// Export a key file's public key as hex.
pub fn export_public_key(path: &Path) -> Result<String> {
    let key_file = load_key(path)?;
    Ok(key_file.public_key_hex)
}

/// Derive the address (first 20 bytes of SHA-256 of public key) for an Ed25519 key.
pub fn derive_address(path: &Path) -> Result<String> {
    let key_file = load_key(path)?;
    if key_file.key_type != KeyType::Ed25519 {
        bail!("address derivation only supported for ed25519 keys");
    }

    let public_bytes = hex::decode(&key_file.public_key_hex)?;
    let hash = sha2_hash(&public_bytes);
    Ok(format!("0x{}", hex::encode(&hash[..20])))
}

fn sha2_hash(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn write_key_file_secure(output_path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(output_path)?;
        file.write_all(contents.as_bytes())?;
    }

    #[cfg(not(unix))]
    {
        fs::write(output_path, contents)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generate_and_load_ed25519() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.key");

        let generated = generate_key(KeyType::Ed25519, &path).unwrap();
        assert_eq!(generated.key_type, KeyType::Ed25519);
        assert_eq!(generated.public_key_hex.len(), 64); // 32 bytes hex

        let loaded = load_key(&path).unwrap();
        assert_eq!(loaded.public_key_hex, generated.public_key_hex);
        assert_eq!(loaded.secret_key_hex, generated.secret_key_hex);
    }

    #[test]
    fn import_from_hex() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("imported.key");

        // Generate a key first to get valid secret bytes
        let original = Keypair::generate();
        let secret_hex = hex::encode(original.secret_key());

        let imported = import_key(KeyType::Ed25519, &secret_hex, &path).unwrap();
        assert_eq!(imported.public_key_hex, hex::encode(original.public_key()));
    }

    #[test]
    fn export_public_key_works() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.key");

        let generated = generate_key(KeyType::Ed25519, &path).unwrap();
        let exported = export_public_key(&path).unwrap();
        assert_eq!(exported, generated.public_key_hex);
    }

    #[test]
    fn derive_address_works() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.key");

        generate_key(KeyType::Ed25519, &path).unwrap();
        let address = derive_address(&path).unwrap();
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn reject_invalid_version() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.key");

        let bad = serde_json::json!({
            "version": 99,
            "key_type": "ed25519",
            "public_key_hex": "aa".repeat(32),
            "secret_key_hex": "bb".repeat(32),
        });
        fs::write(&path, bad.to_string()).unwrap();

        assert!(load_key(&path).is_err());
    }

    #[test]
    fn reject_wrong_key_length() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("bad.key");

        let result = import_key(KeyType::Ed25519, "aabb", &path);
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn key_file_permissions_are_restricted() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("secure.key");
        generate_key(KeyType::Ed25519, &path).unwrap();

        let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn key_type_strategy() -> impl Strategy<Value = KeyType> {
            prop_oneof![
                Just(KeyType::Ed25519),
                Just(KeyType::Vrf),
                Just(KeyType::Bls),
            ]
        }

        proptest! {
            /// Generated keys always roundtrip through save/load.
            #[test]
            fn generate_load_roundtrip(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");

                let generated = generate_key(key_type, &path).unwrap();
                let loaded = load_key(&path).unwrap();

                prop_assert_eq!(&loaded.public_key_hex, &generated.public_key_hex);
                prop_assert_eq!(&loaded.secret_key_hex, &generated.secret_key_hex);
                prop_assert_eq!(loaded.version, 1);
            }

            /// Public key hex is always 64 chars (32 bytes).
            #[test]
            fn public_key_length_invariant(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");

                let kf = generate_key(key_type, &path).unwrap();
                prop_assert_eq!(kf.public_key_hex.len(), 64);
                // Must be valid hex
                prop_assert!(hex::decode(&kf.public_key_hex).is_ok());
            }

            /// Secret key hex is always valid hex of appropriate length.
            #[test]
            fn secret_key_hex_valid(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");

                let kf = generate_key(key_type, &path).unwrap();
                let decoded = hex::decode(&kf.secret_key_hex).unwrap();
                prop_assert!(decoded.len() == 32 || decoded.len() == 64);
            }

            /// Import with 0x prefix and without both yield same key.
            #[test]
            fn import_0x_prefix_invariant(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let keypair = Keypair::generate();
                let secret_hex = hex::encode(keypair.secret_key());

                let path1 = tmp.path().join("no_prefix.key");
                let kf1 = import_key(key_type, &secret_hex, &path1).unwrap();

                let prefixed = format!("0x{}", secret_hex);
                let path2 = tmp.path().join("with_prefix.key");
                let kf2 = import_key(key_type, &prefixed, &path2).unwrap();

                prop_assert_eq!(&kf1.public_key_hex, &kf2.public_key_hex);
                prop_assert_eq!(&kf1.secret_key_hex, &kf2.secret_key_hex);
            }

            /// Import roundtrip: import a generated key's secret, get same public key.
            #[test]
            fn import_preserves_public_key(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let keypair = Keypair::generate();
                let secret_hex = hex::encode(keypair.secret_key());
                let expected_pub = hex::encode(keypair.public_key());

                let path = tmp.path().join("imported.key");
                let kf = import_key(key_type, &secret_hex, &path).unwrap();
                prop_assert_eq!(&kf.public_key_hex, &expected_pub);
            }

            /// Invalid hex is rejected on import.
            #[test]
            fn import_rejects_non_hex(
                garbage in "[^0-9a-fA-Fx][^0-9a-fA-F]{63}",
                key_type in key_type_strategy()
            ) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("bad.key");
                prop_assert!(import_key(key_type, &garbage, &path).is_err());
            }

            /// Wrong-length secret is rejected on import.
            #[test]
            fn import_rejects_wrong_length(
                len in (0usize..64).prop_filter("not 32", |l| *l != 32),
                key_type in key_type_strategy()
            ) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("bad.key");
                let hex_str = "ab".repeat(len);
                prop_assert!(import_key(key_type, &hex_str, &path).is_err());
            }

            /// Address derivation is deterministic for the same key.
            #[test]
            fn address_derivation_deterministic(_i in 0u32..10) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");
                generate_key(KeyType::Ed25519, &path).unwrap();

                let addr1 = derive_address(&path).unwrap();
                let addr2 = derive_address(&path).unwrap();
                prop_assert_eq!(&addr1, &addr2);
                prop_assert!(addr1.starts_with("0x"));
                prop_assert_eq!(addr1.len(), 42);
            }

            /// Address derivation rejects non-ed25519 key types.
            #[test]
            fn address_derivation_rejects_non_ed25519(
                key_type in prop_oneof![Just(KeyType::Vrf), Just(KeyType::Bls)]
            ) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");
                generate_key(key_type, &path).unwrap();
                prop_assert!(derive_address(&path).is_err());
            }

            /// export_public_key matches what was generated.
            #[test]
            fn export_matches_generate(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");
                let generated = generate_key(key_type, &path).unwrap();
                let exported = export_public_key(&path).unwrap();
                prop_assert_eq!(&exported, &generated.public_key_hex);
            }

            /// KeyFile JSON roundtrip through serde.
            #[test]
            fn keyfile_serde_roundtrip(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("test.key");
                let generated = generate_key(key_type, &path).unwrap();

                let json = serde_json::to_string(&generated).unwrap();
                let deserialized: KeyFile = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(&deserialized.public_key_hex, &generated.public_key_hex);
                prop_assert_eq!(&deserialized.secret_key_hex, &generated.secret_key_hex);
                prop_assert_eq!(deserialized.version, generated.version);
            }

            /// Unsupported versions are rejected.
            #[test]
            fn reject_unsupported_version(version in 2u32..1000) {
                let tmp = TempDir::new().unwrap();
                let path = tmp.path().join("bad.key");
                let bad = serde_json::json!({
                    "version": version,
                    "key_type": "ed25519",
                    "public_key_hex": "aa".repeat(32),
                    "secret_key_hex": "bb".repeat(32),
                });
                fs::write(&path, bad.to_string()).unwrap();
                prop_assert!(load_key(&path).is_err());
            }

            /// Each generated key is unique (different public keys).
            #[test]
            fn generated_keys_are_unique(key_type in key_type_strategy()) {
                let tmp = TempDir::new().unwrap();
                let path1 = tmp.path().join("k1.key");
                let path2 = tmp.path().join("k2.key");
                let k1 = generate_key(key_type, &path1).unwrap();
                let k2 = generate_key(key_type, &path2).unwrap();
                prop_assert_ne!(&k1.public_key_hex, &k2.public_key_hex);
            }

            /// KeyType Display roundtrips through serde.
            #[test]
            fn key_type_display(key_type in key_type_strategy()) {
                let display = format!("{}", key_type);
                let json = format!("\"{}\"", display);
                let deserialized: KeyType = serde_json::from_str(&json).unwrap();
                prop_assert_eq!(deserialized, key_type);
            }
        }
    }
}
