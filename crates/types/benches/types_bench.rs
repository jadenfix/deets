use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_types::{
    Address, Block, PublicKey, Signature, Transaction, TransactionReceipt,
    TransactionStatus, VrfProof, H256,
};
use std::collections::HashSet;

fn make_transaction(nonce: u64, data_size: usize) -> Transaction {
    Transaction {
        nonce,
        chain_id: 100,
        sender: Address::from_slice(&[0xAA; 20]).unwrap(),
        sender_pubkey: PublicKey::from_bytes(vec![0xBB; 32]),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: Some(H256([0x01; 32])),
        data: vec![0u8; data_size],
        gas_limit: 21_000,
        fee: 1_000_000,
        signature: Signature::from_bytes(vec![0xCC; 64]),
    }
}

fn make_block(tx_count: usize) -> Block {
    let txs: Vec<Transaction> = (0..tx_count).map(|i| make_transaction(i as u64, 32)).collect();
    Block::new(
        42,
        H256::zero(),
        Address::from_slice(&[0x11; 20]).unwrap(),
        VrfProof {
            output: [0xDD; 32],
            proof: vec![0xEE; 80],
        },
        txs,
    )
}

fn make_receipt() -> TransactionReceipt {
    TransactionReceipt {
        tx_hash: H256([0xAA; 32]),
        block_hash: H256([0xBB; 32]),
        slot: 100,
        status: TransactionStatus::Success,
        gas_used: 21_000,
        logs: vec![],
        state_root: H256([0xCC; 32]),
    }
}

// -- Transaction serialization benchmarks --

fn bench_tx_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_serialize");
    for data_size in [0, 64, 1024, 8192] {
        let tx = make_transaction(0, data_size);
        group.bench_with_input(
            BenchmarkId::new("bincode", data_size),
            &tx,
            |b, tx| b.iter(|| bincode::serialize(black_box(tx)).unwrap()),
        );
    }
    group.finish();
}

fn bench_tx_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_deserialize");
    for data_size in [0, 64, 1024, 8192] {
        let tx = make_transaction(0, data_size);
        let bytes = bincode::serialize(&tx).unwrap();
        group.bench_with_input(
            BenchmarkId::new("bincode", data_size),
            &bytes,
            |b, bytes| b.iter(|| bincode::deserialize::<Transaction>(black_box(bytes)).unwrap()),
        );
    }
    group.finish();
}

fn bench_tx_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_hash");
    for data_size in [0, 64, 1024] {
        let tx = make_transaction(0, data_size);
        group.bench_with_input(BenchmarkId::new("sha256", data_size), &tx, |b, tx| {
            b.iter(|| black_box(tx).hash())
        });
    }
    group.finish();
}

// -- Block serialization benchmarks --

fn bench_block_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_serialize");
    for tx_count in [0, 10, 100, 500] {
        let block = make_block(tx_count);
        group.bench_with_input(
            BenchmarkId::new("bincode", tx_count),
            &block,
            |b, block| b.iter(|| bincode::serialize(black_box(block)).unwrap()),
        );
    }
    group.finish();
}

fn bench_block_deserialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_deserialize");
    for tx_count in [0, 10, 100, 500] {
        let block = make_block(tx_count);
        let bytes = bincode::serialize(&block).unwrap();
        group.bench_with_input(
            BenchmarkId::new("bincode", tx_count),
            &bytes,
            |b, bytes| b.iter(|| bincode::deserialize::<Block>(black_box(bytes)).unwrap()),
        );
    }
    group.finish();
}

fn bench_block_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_hash");
    for tx_count in [0, 10, 100] {
        let block = make_block(tx_count);
        group.bench_with_input(BenchmarkId::new("sha256", tx_count), &block, |b, block| {
            b.iter(|| black_box(block).hash())
        });
    }
    group.finish();
}

// -- Receipt serialization --

fn bench_receipt_roundtrip(c: &mut Criterion) {
    let receipt = make_receipt();
    let bytes = bincode::serialize(&receipt).unwrap();
    c.bench_function("receipt_serialize", |b| {
        b.iter(|| bincode::serialize(black_box(&receipt)).unwrap())
    });
    c.bench_function("receipt_deserialize", |b| {
        b.iter(|| bincode::deserialize::<TransactionReceipt>(black_box(&bytes)).unwrap())
    });
}

// -- Transaction conflict detection --

fn bench_tx_conflicts(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_conflicts");
    // No conflict (disjoint read/write sets)
    let tx_a = {
        let mut tx = make_transaction(0, 0);
        tx.writes = [Address::from_slice(&[0x01; 20]).unwrap()].into();
        tx.reads = [Address::from_slice(&[0x02; 20]).unwrap()].into();
        tx
    };
    let tx_b = {
        let mut tx = make_transaction(1, 0);
        tx.writes = [Address::from_slice(&[0x03; 20]).unwrap()].into();
        tx.reads = [Address::from_slice(&[0x04; 20]).unwrap()].into();
        tx
    };
    group.bench_function("no_conflict", |b| {
        b.iter(|| black_box(&tx_a).conflicts_with(black_box(&tx_b)))
    });

    // Write-write conflict
    let tx_c = {
        let mut tx = make_transaction(2, 0);
        tx.writes = [Address::from_slice(&[0x01; 20]).unwrap()].into();
        tx
    };
    group.bench_function("write_write", |b| {
        b.iter(|| black_box(&tx_a).conflicts_with(black_box(&tx_c)))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_tx_serialize,
    bench_tx_deserialize,
    bench_tx_hash,
    bench_block_serialize,
    bench_block_deserialize,
    bench_block_hash,
    bench_receipt_roundtrip,
    bench_tx_conflicts,
);
criterion_main!(benches);
