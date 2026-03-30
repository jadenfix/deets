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
}
