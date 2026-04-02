use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_crypto_primitives::ed25519::{verify, verify_batch, Keypair};
use aether_crypto_primitives::hash::{blake3_hash, hash_multiple, sha256};

// ---------------------------------------------------------------------------
// Ed25519 benchmarks
// ---------------------------------------------------------------------------

fn bench_ed25519_keygen(c: &mut Criterion) {
    c.bench_function("ed25519/keygen", |b| {
        b.iter(|| black_box(Keypair::generate()));
    });
}

fn bench_ed25519_sign(c: &mut Criterion) {
    let kp = Keypair::generate();
    let msg = [0xABu8; 64];
    c.bench_function("ed25519/sign_64B", |b| {
        b.iter(|| black_box(kp.sign(black_box(&msg))));
    });
}

fn bench_ed25519_verify(c: &mut Criterion) {
    let kp = Keypair::generate();
    let msg = [0xABu8; 64];
    let sig = kp.sign(&msg);
    let pk = kp.public_key();
    c.bench_function("ed25519/verify_64B", |b| {
        b.iter(|| {
            let _ = verify(black_box(&pk), black_box(&msg), black_box(&sig));
        });
    });
}

fn bench_ed25519_batch_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("ed25519/batch_verify");

    for n in [10, 50, 100, 500] {
        let verifications: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = (0..n)
            .map(|i| {
                let kp = Keypair::generate();
                let msg = format!("bench msg {i}").into_bytes();
                let sig = kp.sign(&msg);
                let pk = kp.public_key();
                (pk, msg, sig)
            })
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &verifications, |b, v| {
            b.iter(|| black_box(verify_batch(black_box(v)).unwrap()));
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Hashing benchmarks
// ---------------------------------------------------------------------------

fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha256");
    for size in [32, 256, 1024, 8192] {
        let data = vec![0xCDu8; size];
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            b.iter(|| black_box(sha256(black_box(d))));
        });
    }
    group.finish();
}

fn bench_blake3(c: &mut Criterion) {
    let mut group = c.benchmark_group("blake3");
    for size in [32, 256, 1024, 8192] {
        let data = vec![0xCDu8; size];
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, d| {
            b.iter(|| black_box(blake3_hash(black_box(d))));
        });
    }
    group.finish();
}

fn bench_hash_multiple(c: &mut Criterion) {
    let chunks: Vec<Vec<u8>> = (0..16).map(|i| vec![i as u8; 64]).collect();
    let refs: Vec<&[u8]> = chunks.iter().map(|c| c.as_slice()).collect();
    c.bench_function("sha256/hash_multiple_16x64B", |b| {
        b.iter(|| black_box(hash_multiple(black_box(&refs))));
    });
}

criterion_group!(
    benches,
    bench_ed25519_keygen,
    bench_ed25519_sign,
    bench_ed25519_verify,
    bench_ed25519_batch_verify,
    bench_sha256,
    bench_blake3,
    bench_hash_multiple,
);
criterion_main!(benches);
