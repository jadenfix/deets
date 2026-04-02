use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_crypto_primitives::Keypair;
use aether_mempool::Mempool;
use aether_types::{PublicKey, Signature, Transaction};
use std::collections::HashSet;

fn make_tx(kp: &Keypair, nonce: u64, fee: u128, chain_id: u64) -> Transaction {
    let sender_pubkey = PublicKey::from_bytes(kp.public_key().to_vec());
    let sender = sender_pubkey.to_address();
    let mut tx = Transaction {
        nonce,
        chain_id,
        sender,
        sender_pubkey,
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(kp.sign(hash.as_bytes()));
    tx
}

/// Benchmark: admit N transactions from N distinct senders (1 tx/sender).
fn bench_admission_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool/admission");
    let chain_id = 900u64;

    for n in [100usize, 500, 1000, 5000] {
        // Pre-generate keypairs and txs outside the measured region
        let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::generate()).collect();
        let txs: Vec<Transaction> = keypairs
            .iter()
            .enumerate()
            .map(|(i, kp)| make_tx(kp, 0, 60_000 + i as u128, chain_id))
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut mempool = Mempool::with_defaults();
                for tx in &txs {
                    let _ = mempool.add_transaction(black_box(tx.clone()));
                }
                black_box(mempool.len())
            });
        });
    }
    group.finish();
}

/// Benchmark: get_transactions (block-building) from a pre-filled mempool.
///
/// Since Mempool doesn't implement Clone, we rebuild each iteration with a
/// per-iteration seed offset so the compiler can't optimize it away.
fn bench_block_building(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool/block_building");
    let chain_id = 900u64;

    for pool_size in [100usize, 500, 1000] {
        let keypairs: Vec<Keypair> = (0..pool_size).map(|_| Keypair::generate()).collect();
        let txs: Vec<Transaction> = keypairs
            .iter()
            .enumerate()
            .map(|(i, kp)| make_tx(kp, 0, 60_000 + i as u128, chain_id))
            .collect();

        for batch in [50usize, 200] {
            let label = format!("pool={pool_size}/batch={batch}");
            group.bench_function(&label, |b| {
                b.iter(|| {
                    let mut mp = Mempool::with_defaults();
                    for tx in &txs {
                        let _ = mp.add_transaction(tx.clone());
                    }
                    black_box(mp.get_transactions(black_box(batch), black_box(1_000_000)))
                });
            });
        }
    }
    group.finish();
}

/// Benchmark: advance_sender_nonce for N senders after block application.
fn bench_nonce_advancement(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool/nonce_advance");
    let chain_id = 900u64;

    for n in [100usize, 500, 1000] {
        let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::generate()).collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut mp = Mempool::with_defaults();
                for kp in &keypairs {
                    let tx0 = make_tx(kp, 0, 60_000, chain_id);
                    let sender = tx0.sender;
                    let tx1 = make_tx(kp, 1, 60_000, chain_id);
                    let _ = mp.add_transaction(tx0);
                    let _ = mp.add_transaction(tx1);
                    mp.set_sender_nonce(sender, 0);
                }
                // Simulate block application: advance every sender past nonce 0
                for kp in &keypairs {
                    let sender_pubkey = PublicKey::from_bytes(kp.public_key().to_vec());
                    let sender = sender_pubkey.to_address();
                    mp.advance_sender_nonce(black_box(sender), black_box(1));
                }
                black_box(mp.len())
            });
        });
    }
    group.finish();
}

/// Benchmark: remove_transactions by hash (post-block cleanup).
fn bench_remove_transactions(c: &mut Criterion) {
    let mut group = c.benchmark_group("mempool/remove");
    let chain_id = 900u64;

    for n in [50usize, 200, 500] {
        let keypairs: Vec<Keypair> = (0..n).map(|_| Keypair::generate()).collect();
        let txs: Vec<Transaction> = keypairs
            .iter()
            .enumerate()
            .map(|(i, kp)| make_tx(kp, 0, 60_000 + i as u128, chain_id))
            .collect();
        let hashes: Vec<_> = txs.iter().map(|t| t.hash()).collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let mut mp = Mempool::with_defaults();
                for tx in &txs {
                    let _ = mp.add_transaction(tx.clone());
                }
                mp.remove_transactions(black_box(&hashes));
                black_box(mp.len())
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_admission_throughput,
    bench_block_building,
    bench_nonce_advancement,
    bench_remove_transactions,
);
criterion_main!(benches);
