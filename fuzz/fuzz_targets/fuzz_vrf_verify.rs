#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz VRF proof verification — must never panic
    if data.len() < 32 + 80 + 32 {
        return;
    }
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&data[..32]);
    let proof_bytes = data[32..112].to_vec();
    let mut output = [0u8; 32];
    output.copy_from_slice(&data[112..144]);
    let input = &data[144..];

    let proof = aether_crypto_vrf::VrfProof {
        proof: proof_bytes,
        output,
    };
    let _ = aether_crypto_vrf::verify_proof(&pubkey, input, &proof);
});
