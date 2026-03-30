#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz Merkle proof verification — must never panic
    if data.len() < 20 + 32 + 32 {
        return;
    }
    let key = aether_types::Address::from_slice(&data[..20]).unwrap();
    let mut value_bytes = [0u8; 32];
    value_bytes.copy_from_slice(&data[20..52]);
    let value_hash = aether_types::H256::from_slice(&value_bytes).unwrap();
    let mut root_bytes = [0u8; 32];
    root_bytes.copy_from_slice(&data[52..84]);
    let root = aether_types::H256::from_slice(&root_bytes).unwrap();

    // Build siblings from remaining data (chunks of 32 bytes)
    let remaining = &data[84..];
    let num_siblings = remaining.len() / 32;
    let siblings: Vec<aether_types::H256> = (0..num_siblings)
        .map(|i| {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&remaining[i * 32..(i + 1) * 32]);
            aether_types::H256::from_slice(&bytes).unwrap()
        })
        .collect();

    let proof = aether_state_merkle::MerkleProof::new(key, Some(value_hash), root, siblings);
    let _ = proof.verify();
});
