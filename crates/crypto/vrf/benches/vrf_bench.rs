use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aether_crypto_vrf::{verify_proof, VrfKeypair};

fn bench_vrf_prove(c: &mut Criterion) {
    let keypair = VrfKeypair::generate();
    let input = b"benchmark input for VRF prove";

    c.bench_function("vrf_prove", |b| b.iter(|| keypair.prove(black_box(input))));
}

fn bench_vrf_verify(c: &mut Criterion) {
    let keypair = VrfKeypair::generate();
    let input = b"benchmark input for VRF verify";
    let proof = keypair.prove(input);

    c.bench_function("vrf_verify", |b| {
        b.iter(|| {
            verify_proof(
                black_box(keypair.public_key()),
                black_box(input),
                black_box(&proof),
            )
        })
    });
}

fn bench_vrf_prove_and_verify(c: &mut Criterion) {
    let keypair = VrfKeypair::generate();
    let input = b"benchmark input for VRF full cycle";

    c.bench_function("vrf_prove_and_verify", |b| {
        b.iter(|| {
            let proof = keypair.prove(black_box(input));
            verify_proof(keypair.public_key(), input, &proof).unwrap()
        })
    });
}

criterion_group!(
    benches,
    bench_vrf_prove,
    bench_vrf_verify,
    bench_vrf_prove_and_verify
);
criterion_main!(benches);
