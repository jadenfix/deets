use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_types::{Address, PublicKey, Signature, Transaction};
use std::collections::HashSet;

fn make_tx(reads: &[u8], writes: &[u8]) -> Transaction {
    let read_addrs: HashSet<Address> = reads
        .iter()
        .map(|&b| Address::from_slice(&[b; 20]).unwrap())
        .collect();
    let write_addrs: HashSet<Address> = writes
        .iter()
        .map(|&b| Address::from_slice(&[b; 20]).unwrap())
        .collect();

    Transaction {
        nonce: 0,
        chain_id: 1,
        sender: Address::from_slice(&[1u8; 20]).unwrap(),
        sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
        inputs: vec![],
        outputs: vec![],
        reads: read_addrs,
        writes: write_addrs,
        program_id: None,
        data: vec![],
        gas_limit: 21000,
        fee: 1000,
        signature: Signature::from_bytes(vec![0u8; 64]),
    }
}

/// Non-conflicting transactions: each writes to a unique address.
fn independent_txs(n: usize) -> Vec<Transaction> {
    (0..n).map(|i| make_tx(&[], &[(i % 256) as u8])).collect()
}

/// Fully conflicting: all transactions write to address 0.
fn conflicting_txs(n: usize) -> Vec<Transaction> {
    (0..n).map(|_| make_tx(&[], &[0])).collect()
}

/// Chain of dependencies: tx[i] reads what tx[i-1] wrote.
fn chain_txs(n: usize) -> Vec<Transaction> {
    (0..n)
        .map(|i| {
            if i == 0 {
                make_tx(&[], &[0])
            } else {
                make_tx(&[((i - 1) % 256) as u8], &[(i % 256) as u8])
            }
        })
        .collect()
}

fn bench_schedule(c: &mut Criterion) {
    let scheduler = aether_runtime::ParallelScheduler::new();

    let mut group = c.benchmark_group("schedule");
    for size in [10, 50, 100, 500] {
        group.bench_with_input(BenchmarkId::new("independent", size), &size, |b, &size| {
            let txs = independent_txs(size);
            b.iter(|| black_box(scheduler.schedule(black_box(&txs))));
        });
        group.bench_with_input(BenchmarkId::new("conflicting", size), &size, |b, &size| {
            let txs = conflicting_txs(size);
            b.iter(|| black_box(scheduler.schedule(black_box(&txs))));
        });
        group.bench_with_input(BenchmarkId::new("chain", size), &size, |b, &size| {
            let txs = chain_txs(size);
            b.iter(|| black_box(scheduler.schedule(black_box(&txs))));
        });
    }
    group.finish();
}

fn bench_execute_parallel(c: &mut Criterion) {
    let scheduler = aether_runtime::ParallelScheduler::new();

    let mut group = c.benchmark_group("execute_parallel");
    for size in [10, 50, 100, 500] {
        let txs = independent_txs(size);
        let batches = scheduler.schedule(&txs);

        group.bench_with_input(
            BenchmarkId::new("independent", size),
            &batches,
            |b, batches| {
                b.iter(|| {
                    scheduler
                        .execute_parallel(batches.clone(), |tx| {
                            black_box(tx);
                            Ok(())
                        })
                        .unwrap();
                });
            },
        );
    }
    group.finish();
}

fn bench_execute_sequential(c: &mut Criterion) {
    let scheduler = aether_runtime::ParallelScheduler::new();

    let mut group = c.benchmark_group("execute_sequential");
    for size in [10, 50, 100, 500] {
        let txs = independent_txs(size);
        let batches = scheduler.schedule(&txs);

        group.bench_with_input(
            BenchmarkId::new("independent", size),
            &batches,
            |b, batches| {
                b.iter(|| {
                    scheduler
                        .execute_sequential(batches.clone(), |tx| {
                            black_box(tx);
                            Ok(())
                        })
                        .unwrap();
                });
            },
        );
    }
    group.finish();
}

fn bench_speedup_estimate(c: &mut Criterion) {
    let scheduler = aether_runtime::ParallelScheduler::new();

    let mut group = c.benchmark_group("speedup_estimate");
    for size in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("independent", size), &size, |b, &size| {
            let txs = independent_txs(size);
            b.iter(|| black_box(scheduler.speedup_estimate(black_box(&txs))));
        });
        group.bench_with_input(BenchmarkId::new("conflicting", size), &size, |b, &size| {
            let txs = conflicting_txs(size);
            b.iter(|| black_box(scheduler.speedup_estimate(black_box(&txs))));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_schedule,
    bench_execute_parallel,
    bench_execute_sequential,
    bench_speedup_estimate,
);
criterion_main!(benches);
