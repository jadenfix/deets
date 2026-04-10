use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use aether_da_erasure::{ReedSolomonDecoder, ReedSolomonEncoder};

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("rs_encode");

    // Vary payload size with a realistic RS(10,2) config
    for size in [1_024, 4_096, 32_768, 262_144, 1_048_576] {
        let data = vec![0xABu8; size];
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| encoder.encode(black_box(data)).unwrap());
        });
    }

    group.finish();
}

fn bench_decode_full(c: &mut Criterion) {
    let mut group = c.benchmark_group("rs_decode_full");

    for size in [1_024, 4_096, 32_768, 262_144] {
        let data = vec![0xCDu8; size];
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();
        let decoder = ReedSolomonDecoder::new(10, 2).unwrap();
        let shards: Vec<Option<Vec<u8>>> = encoder
            .encode(&data)
            .unwrap()
            .into_iter()
            .map(Some)
            .collect();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &shards, |b, shards| {
            b.iter(|| decoder.decode(black_box(shards)).unwrap());
        });
    }

    group.finish();
}

fn bench_decode_recovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("rs_decode_recovery");

    for size in [1_024, 4_096, 32_768, 262_144] {
        let data = vec![0xEFu8; size];
        let encoder = ReedSolomonEncoder::new(10, 2).unwrap();
        let decoder = ReedSolomonDecoder::new(10, 2).unwrap();
        let mut shards: Vec<Option<Vec<u8>>> = encoder
            .encode(&data)
            .unwrap()
            .into_iter()
            .map(Some)
            .collect();
        // Drop 2 shards (max recoverable for r=2)
        shards[0] = None;
        shards[5] = None;

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &shards, |b, shards| {
            b.iter(|| decoder.decode(black_box(shards)).unwrap());
        });
    }

    group.finish();
}

fn bench_encode_shard_configs(c: &mut Criterion) {
    let mut group = c.benchmark_group("rs_encode_configs");
    let data = vec![0x42u8; 32_768];

    for (k, r) in [(4, 2), (10, 2), (10, 4), (16, 4), (32, 8)] {
        let encoder = ReedSolomonEncoder::new(k, r).unwrap();
        let label = format!("RS({},{})", k + r, k);

        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(BenchmarkId::new("32KB", &label), &data, |b, data| {
            b.iter(|| encoder.encode(black_box(data)).unwrap());
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_encode,
    bench_decode_full,
    bench_decode_recovery,
    bench_encode_shard_configs,
);
criterion_main!(benches);
