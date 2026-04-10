use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_state_storage::{
    pruning, Storage, StorageBatch, CF_ACCOUNTS, CF_BLOCKS, CF_METADATA, CF_RECEIPTS, CF_UTXOS,
};
use tempfile::TempDir;

/// Pre-populate a storage instance with `count` entries in the given CF.
/// Keys are 8-byte big-endian integers; values are 128-byte payloads.
fn populate(storage: &Storage, cf: &str, count: usize) {
    let mut batch = StorageBatch::new();
    let value = vec![0xABu8; 128];
    for i in 0u64..count as u64 {
        batch.put(cf, i.to_be_bytes().to_vec(), value.clone());
    }
    storage.write_batch(batch).unwrap();
}

/// Benchmark: point reads (get) from a warm cache at various DB sizes.
fn bench_point_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/point_read");

    for n in [1_000usize, 10_000, 100_000] {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::open(tmp.path()).unwrap();
        populate(&storage, CF_ACCOUNTS, n);

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let key = ((n / 2) as u64).to_be_bytes(); // mid-range key
            b.iter(|| {
                storage
                    .get(black_box(CF_ACCOUNTS), black_box(&key))
                    .unwrap()
            });
        });
    }
    group.finish();
}

/// Benchmark: single put (point write) into an existing database.
fn bench_point_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/point_write");

    for n in [1_000usize, 10_000] {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::open(tmp.path()).unwrap();
        populate(&storage, CF_ACCOUNTS, n);
        let value = vec![0xFFu8; 128];

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let mut counter = n as u64;
            b.iter(|| {
                let key = counter.to_be_bytes();
                counter += 1;
                storage
                    .put(black_box(CF_ACCOUNTS), black_box(&key), black_box(&value))
                    .unwrap();
            });
        });
    }
    group.finish();
}

/// Benchmark: batch write of N entries atomically.
fn bench_batch_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/batch_write");
    let value = vec![0xCCu8; 128];

    for batch_size in [10usize, 100, 500, 1_000] {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::open(tmp.path()).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &batch_size| {
                let mut base = 0u64;
                b.iter(|| {
                    let mut batch = StorageBatch::new();
                    for i in 0..batch_size as u64 {
                        let key = (base + i).to_be_bytes().to_vec();
                        batch.put(CF_BLOCKS, key, value.clone());
                    }
                    base += batch_size as u64;
                    storage.write_batch(black_box(batch)).unwrap();
                });
            },
        );
    }
    group.finish();
}

/// Benchmark: full iteration over all keys in a column family.
fn bench_full_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/full_scan");

    for n in [1_000usize, 10_000] {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::open(tmp.path()).unwrap();
        populate(&storage, CF_METADATA, n);

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let iter = storage.iterator(black_box(CF_METADATA)).unwrap();
                let count = iter.count();
                black_box(count)
            });
        });
    }
    group.finish();
}

/// Benchmark: prefix scan — scan all keys with a 4-byte prefix.
fn bench_prefix_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/prefix_scan");

    // Populate with 10K entries; 1K share the same 4-byte prefix
    let tmp = TempDir::new().unwrap();
    let storage = Storage::open(tmp.path()).unwrap();
    let mut batch = StorageBatch::new();
    let prefix = 0xDEADBEEFu32.to_be_bytes();
    let other_prefix = 0xCAFEBABEu32.to_be_bytes();
    let value = vec![0u8; 64];

    for i in 0u32..1_000 {
        let mut key = prefix.to_vec();
        key.extend_from_slice(&i.to_be_bytes());
        batch.put(CF_UTXOS, key, value.clone());
    }
    for i in 0u32..9_000 {
        let mut key = other_prefix.to_vec();
        key.extend_from_slice(&i.to_be_bytes());
        batch.put(CF_UTXOS, key, value.clone());
    }
    storage.write_batch(batch).unwrap();

    group.bench_function("prefix_1k_of_10k", |b| {
        b.iter(|| {
            let iter = storage
                .prefix_iterator(black_box(CF_UTXOS), black_box(&prefix))
                .unwrap();
            let count = iter.count();
            black_box(count)
        });
    });

    group.finish();
}

/// Benchmark: block pruning — delete N old block entries.
fn bench_block_pruning(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/block_pruning");

    for prune_count in [100usize, 500, 1_000] {
        let tmp = TempDir::new().unwrap();
        let storage = Storage::open(tmp.path()).unwrap();

        // Write prune_count blocks using the production slot key layout
        let mut batch = StorageBatch::new();
        for slot in 0u64..prune_count as u64 {
            // metadata: "slot:{n}" → hash
            let meta_key = format!("slot:{slot}").into_bytes();
            let hash = [slot as u8; 32].to_vec();
            batch.put(CF_METADATA, meta_key, hash.clone());
            // blocks CF: hash → dummy payload
            batch.put(CF_BLOCKS, hash.clone(), vec![0u8; 256]);
            // receipts CF: hash → dummy receipt
            batch.put(CF_RECEIPTS, hash, vec![0u8; 64]);
        }
        storage.write_batch(batch).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(prune_count),
            &prune_count,
            |b, &prune_count| {
                b.iter(|| {
                    pruning::prune_old_blocks_and_receipts(
                        black_box(&storage),
                        black_box(prune_count as u64),
                    )
                    .unwrap()
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_point_read,
    bench_point_write,
    bench_batch_write,
    bench_full_scan,
    bench_prefix_scan,
    bench_block_pruning,
);
criterion_main!(benches);
