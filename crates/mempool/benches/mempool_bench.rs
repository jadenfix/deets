use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_crypto_primitives::Keypair;
use aether_mempool::Mempool;
use aether_types::{PublicKey, Signature, Transaction, H160};
use std::collections::HashSet;

fn make_tx(keypair: &Keypair, nonce: u64, fee: u128) -> Transaction {
    let address = H160::from_slice(&keypair.to_address()).unwrap();
    let mut tx = Transaction {
        nonce,
        chain_id: 100,
        sender: address,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![0u8; 128],
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

/// Pre-generate keypairs and transactions for benchmarking.
fn generate_txs(sender_count: usize, txs_per_sender: usize) -> Vec<Transaction> {
    let keypairs: Vec<Keypair> = (0..sender_count).map(|_| Keypair::generate()).collect();
    let mut txs = Vec::with_capacity(sender_count * txs_per_sender);
    for kp in &keypairs {
        for n in 0..txs_per_sender {
            // Vary fees so the priority queue has work to do
            let fee = 10_000 + (n as u128) * 100;
            txs.push(make_tx(kp, n as u64, fee));
        }
    }
    txs
}

fn bench_add_transaction(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_add");

    for count in [100, 1_000, 5_000] {
        let txs = generate_txs(count, 1);

        group.bench_with_input(
            BenchmarkId::new("distinct_senders", count),
            &txs,
            |b, txs| {
                b.iter(|| {
                    let mut pool = Mempool::with_defaults();
                    for tx in txs {
                        let _ = pool.add_transaction(black_box(tx.clone()));
                    }
                });
            },
        );
    }

    // Many txs from one sender (nonce ordering path)
    let kp = Keypair::generate();
    let txs: Vec<Transaction> = (0..500)
        .map(|n| make_tx(&kp, n, 10_000 + n as u128 * 50))
        .collect();

    group.bench_function("single_sender_500_nonces", |b| {
        b.iter(|| {
            let mut pool = Mempool::with_defaults();
            for tx in &txs {
                let _ = pool.add_transaction(black_box(tx.clone()));
            }
        });
    });

    group.finish();
}

fn bench_get_transactions(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_get");

    for pool_size in [100, 1_000, 5_000] {
        let txs = generate_txs(pool_size, 1);

        group.bench_with_input(BenchmarkId::new("pack_block", pool_size), &txs, |b, txs| {
            b.iter_batched(
                || {
                    let mut pool = Mempool::with_defaults();
                    for tx in txs {
                        let _ = pool.add_transaction(tx.clone());
                    }
                    pool
                },
                |mut pool| {
                    black_box(pool.get_transactions(500, 10_000_000));
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn bench_remove_transactions(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_remove");

    let txs = generate_txs(1_000, 1);
    let hashes: Vec<_> = txs.iter().map(|tx| tx.hash()).collect();

    group.bench_function("remove_500_from_1000", |b| {
        b.iter_batched(
            || {
                let mut pool = Mempool::with_defaults();
                for tx in &txs {
                    let _ = pool.add_transaction(tx.clone());
                }
                pool
            },
            |mut pool| {
                pool.remove_transactions(black_box(&hashes[..500]));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bench_evict_expired(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool_evict");

    // Insert txs at slot 0, then advance slot past TTL to trigger expiry
    let txs = generate_txs(1_000, 1);

    group.bench_function("expire_1000_stale", |b| {
        b.iter_batched(
            || {
                let mut pool = Mempool::with_defaults();
                for tx in &txs {
                    let _ = pool.add_transaction(tx.clone());
                }
                pool
            },
            |mut pool| {
                // Advance slot past MAX_TX_AGE_SLOTS (1800)
                pool.set_current_slot(black_box(2000));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_add_transaction,
    bench_get_transactions,
    bench_remove_transactions,
    bench_evict_expired,
);
criterion_main!(benches);
