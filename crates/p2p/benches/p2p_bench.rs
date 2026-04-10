use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::collections::HashMap;

use aether_p2p::compact_block::{compress_message, decompress_message, CompactBlock};
use aether_types::*;

fn make_tx(nonce: u64) -> Transaction {
    Transaction {
        nonce,
        chain_id: 1,
        sender: Address::from_slice(&[1u8; 20]).unwrap(),
        sender_pubkey: PublicKey::from_bytes(vec![2u8; 32]),
        inputs: vec![],
        outputs: vec![],
        reads: std::collections::HashSet::new(),
        writes: std::collections::HashSet::new(),
        program_id: None,
        data: vec![0u8; 100],
        gas_limit: 21000,
        fee: 1000,
        signature: Signature::from_bytes(vec![3u8; 64]),
    }
}

fn make_block(num_txs: usize) -> Block {
    let txs: Vec<Transaction> = (0..num_txs).map(|i| make_tx(i as u64)).collect();
    Block::new(
        0,
        H256::zero(),
        Address::from_slice(&[1u8; 20]).unwrap(),
        VrfProof {
            output: [0u8; 32],
            proof: vec![0u8; 80],
        },
        txs,
    )
}

fn bench_compact_block_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact_block_create");
    for num_txs in [10, 100, 500, 1000] {
        let block = make_block(num_txs);
        group.bench_with_input(BenchmarkId::from_parameter(num_txs), &block, |b, block| {
            b.iter(|| CompactBlock::from_block(black_box(block)));
        });
    }
    group.finish();
}

fn bench_compact_block_reconstruct(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact_block_reconstruct");
    for num_txs in [10, 100, 500, 1000] {
        let block = make_block(num_txs);
        let compact = CompactBlock::from_block(&block);
        let known: HashMap<H256, Transaction> = block
            .transactions
            .iter()
            .map(|tx| (tx.hash(), tx.clone()))
            .collect();
        group.bench_with_input(
            BenchmarkId::from_parameter(num_txs),
            &(compact, known),
            |b, (compact, known)| {
                b.iter(|| compact.reconstruct(black_box(known)));
            },
        );
    }
    group.finish();
}

fn bench_compact_block_wire_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("compact_block_wire_size");
    for num_txs in [10, 100, 500] {
        let block = make_block(num_txs);
        let compact = CompactBlock::from_block(&block);
        group.bench_with_input(
            BenchmarkId::from_parameter(num_txs),
            &compact,
            |b, compact| {
                b.iter(|| compact.wire_size());
            },
        );
    }
    group.finish();
}

fn bench_compress_message(c: &mut Criterion) {
    let mut group = c.benchmark_group("compress_message");
    // Realistic: serialized block data (repetitive)
    for size in [256, 1024, 4096, 16384, 65536] {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        group.bench_with_input(BenchmarkId::new("repetitive", size), &data, |b, data| {
            b.iter(|| compress_message(black_box(data)));
        });
    }
    // Random data (worst case for compression)
    for size in [1024, 16384] {
        let data: Vec<u8> = (0..size).map(|i| ((i * 7 + 13) % 256) as u8).collect();
        group.bench_with_input(BenchmarkId::new("pseudorandom", size), &data, |b, data| {
            b.iter(|| compress_message(black_box(data)));
        });
    }
    group.finish();
}

fn bench_decompress_message(c: &mut Criterion) {
    let mut group = c.benchmark_group("decompress_message");
    for size in [256, 1024, 4096, 16384, 65536] {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        let compressed = compress_message(&data);
        group.bench_with_input(BenchmarkId::from_parameter(size), &compressed, |b, comp| {
            b.iter(|| decompress_message(black_box(comp)).unwrap());
        });
    }
    group.finish();
}

fn bench_compress_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("compress_roundtrip");
    for size in [1024, 4096, 16384] {
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| {
                let compressed = compress_message(black_box(data));
                decompress_message(&compressed).unwrap()
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_compact_block_creation,
    bench_compact_block_reconstruct,
    bench_compact_block_wire_size,
    bench_compress_message,
    bench_decompress_message,
    bench_compress_roundtrip,
);
criterion_main!(benches);
