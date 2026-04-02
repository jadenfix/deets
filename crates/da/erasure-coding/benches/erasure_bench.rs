use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_da_erasure::{ReedSolomonDecoder, ReedSolomonEncoder};

/// Aether production erasure-coding configuration: 10 data shards, 2 parity shards.
/// Any 10 of 12 shards suffice for reconstruction (16.7% redundancy overhead).
const DATA_SHARDS: usize = 10;
const PARITY_SHARDS: usize = 2;

/// Block sizes representative of Aether workloads:
/// - 64 KB: small/empty block
/// - 512 KB: medium load
/// - 2 MB: target max block size
const BLOCK_SIZES: &[usize] = &[64 * 1024, 512 * 1024, 2 * 1024 * 1024];

/// Benchmark: encode `data_shards` + `parity_shards` from raw block data.
fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("erasure/encode");
    let encoder = ReedSolomonEncoder::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

    for &size in BLOCK_SIZES {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();

        group.throughput(criterion::Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &data,
            |b, data| {
                b.iter(|| {
                    let shards = encoder.encode(black_box(data)).unwrap();
                    black_box(shards)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark: decode with all shards present (no erasures, fast path).
fn bench_decode_no_erasures(c: &mut Criterion) {
    let mut group = c.benchmark_group("erasure/decode_no_erasures");
    let encoder = ReedSolomonEncoder::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    let decoder = ReedSolomonDecoder::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

    for &size in BLOCK_SIZES {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let shards = encoder.encode(&data).unwrap();
        let present: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

        group.throughput(criterion::Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &present,
            |b, present| {
                b.iter(|| {
                    let recovered = decoder.decode(black_box(present)).unwrap();
                    black_box(recovered)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark: decode with the maximum tolerable erasures (all parity shards lost).
/// This exercises the full Gaussian elimination reconstruction path.
fn bench_decode_max_erasures(c: &mut Criterion) {
    let mut group = c.benchmark_group("erasure/decode_max_erasures");
    let encoder = ReedSolomonEncoder::new(DATA_SHARDS, PARITY_SHARDS).unwrap();
    let decoder = ReedSolomonDecoder::new(DATA_SHARDS, PARITY_SHARDS).unwrap();

    for &size in BLOCK_SIZES {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let shards = encoder.encode(&data).unwrap();

        // Drop all parity shards (maximum tolerable loss for RS(10,2))
        let mut present: Vec<Option<Vec<u8>>> =
            shards.into_iter().map(Some).collect();
        for s in present.iter_mut().skip(DATA_SHARDS) {
            *s = None;
        }

        group.throughput(criterion::Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KB", size / 1024)),
            &present,
            |b, present| {
                b.iter(|| {
                    let recovered = decoder.decode(black_box(present)).unwrap();
                    black_box(recovered)
                });
            },
        );
    }
    group.finish();
}

/// Benchmark: encode with different shard counts (shard-count sensitivity).
fn bench_encode_shard_configs(c: &mut Criterion) {
    let mut group = c.benchmark_group("erasure/encode_shard_configs");
    let data_size = 512 * 1024; // 512 KB
    let data: Vec<u8> = (0..data_size).map(|i| (i % 251) as u8).collect();

    for (data_shards, parity_shards) in [(4, 2), (10, 2), (16, 4), (32, 8)] {
        let encoder = ReedSolomonEncoder::new(data_shards, parity_shards).unwrap();
        let label = format!("RS({data_shards},{parity_shards})");

        group.bench_function(&label, |b| {
            b.iter(|| {
                let shards = encoder.encode(black_box(&data)).unwrap();
                black_box(shards)
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_encode,
    bench_decode_no_erasures,
    bench_decode_max_erasures,
    bench_encode_shard_configs,
);
criterion_main!(benches);
