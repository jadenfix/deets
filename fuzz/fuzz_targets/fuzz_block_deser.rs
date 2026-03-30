#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz Block deserialization — must never panic
    let _ = bincode::deserialize::<aether_types::Block>(data);
});
