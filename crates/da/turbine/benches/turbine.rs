use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use aether_crypto_primitives::Keypair;
use aether_da_shreds::{shred::ShredVariant, Shred};
use aether_types::{Signature, H256};

fn bench_make_shreds(c: &mut Criterion) {
    use aether_da_turbine::TurbineBroadcaster;

    let mut group = c.benchmark_group("turbine_make_shreds");

    for size in [1_024, 4_096, 32_768, 262_144, 1_048_576] {
        let payload = vec![0xABu8; size];
        let broadcaster = TurbineBroadcaster::new(10, 2, 1, Keypair::generate()).unwrap();
        let block_id = H256::zero();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, payload| {
            b.iter(|| {
                broadcaster
                    .make_shreds(black_box(1), black_box(block_id), black_box(payload))
                    .unwrap()
            });
        });
    }

    group.finish();
}

fn bench_ingest_shreds(c: &mut Criterion) {
    use aether_da_turbine::{TurbineBroadcaster, TurbineReceiver};

    let mut group = c.benchmark_group("turbine_ingest");

    for size in [1_024, 4_096, 32_768, 262_144] {
        let payload = vec![0xCDu8; size];
        let broadcaster = TurbineBroadcaster::new(10, 2, 1, Keypair::generate()).unwrap();
        let shreds = broadcaster.make_shreds(1, H256::zero(), &payload).unwrap();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &shreds, |b, shreds| {
            b.iter(|| {
                let mut receiver = TurbineReceiver::new(10, 2).unwrap();
                for shred in shreds.iter() {
                    let _ = receiver.ingest_shred(black_box(shred.clone()));
                }
            });
        });
    }

    group.finish();
}

fn bench_shred_hash_payload(c: &mut Criterion) {
    let mut group = c.benchmark_group("shred_hash_payload");

    for size in [64, 256, 1_024, 4_096] {
        let payload = vec![0x55u8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, payload| {
            b.iter(|| Shred::hash_payload(black_box(payload)));
        });
    }

    group.finish();
}

fn bench_shred_signing_message(c: &mut Criterion) {
    let payload = vec![0xABu8; 1024];
    let payload_hash = Shred::hash_payload(&payload);
    let shred = Shred::new(
        ShredVariant::Data,
        42,
        0,
        1,
        0,
        H256::zero(),
        payload,
        Signature::from_bytes(vec![0; 64]),
    );

    c.bench_function("shred_signing_message", |b| {
        b.iter(|| black_box(&shred).signing_message());
    });

    c.bench_function("shred_build_signing_message", |b| {
        b.iter(|| {
            Shred::build_signing_message(black_box(42), black_box(0), black_box(&payload_hash))
        });
    });
}

criterion_group!(
    benches,
    bench_make_shreds,
    bench_ingest_shreds,
    bench_shred_hash_payload,
    bench_shred_signing_message,
);
criterion_main!(benches);
