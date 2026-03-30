#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz Transaction deserialization — must never panic
    let _ = bincode::deserialize::<aether_types::Transaction>(data);
});
