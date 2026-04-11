use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_node::{compute_receipts_root, compute_transactions_root};
use aether_state_storage::Storage;
use aether_types::{
    Address, Block, PublicKey, Signature, Transaction, TransactionReceipt, TransactionStatus,
    TransferPayload, VrfProof, H160, H256, TRANSFER_PROGRAM_ID,
};
use std::collections::HashSet;

fn make_vrf_proof() -> VrfProof {
    VrfProof {
        output: [0xABu8; 32],
        proof: vec![0u8; 64],
    }
}

fn make_signed_tx(keypair: &Keypair, nonce: u64, fee: u128) -> Transaction {
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
        data: vec![],
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

fn make_transfer_tx(
    keypair: &Keypair,
    recipient: Address,
    amount: u128,
    nonce: u64,
    fee: u128,
) -> Transaction {
    let address = H160::from_slice(&keypair.to_address()).unwrap();
    let payload = TransferPayload {
        recipient,
        amount,
        memo: None,
    };
    let data = bincode::serialize(&payload).unwrap();
    let mut tx = Transaction {
        nonce,
        chain_id: 100,
        sender: address,
        sender_pubkey: PublicKey::from_bytes(keypair.public_key()),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: Some(TRANSFER_PROGRAM_ID),
        data,
        gas_limit: 21_000,
        fee,
        signature: Signature::from_bytes(vec![]),
    };
    let hash = tx.hash();
    tx.signature = Signature::from_bytes(keypair.sign(hash.as_bytes()));
    tx
}

fn make_receipt(i: u64) -> TransactionReceipt {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..8].copy_from_slice(&i.to_le_bytes());
    TransactionReceipt {
        tx_hash: H256::from_slice(&hash_bytes).unwrap(),
        block_hash: H256::zero(),
        slot: 1,
        status: TransactionStatus::Success,
        gas_used: 21_000,
        logs: vec![aether_types::transaction::Log {
            address: H160::from_slice(&[0x01; 20]).unwrap(),
            topics: vec![H256::zero()],
            data: vec![0u8; 32],
        }],
        state_root: H256::zero(),
    }
}

fn temp_ledger() -> Ledger {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let storage = Storage::open(&path).unwrap();
    std::mem::forget(dir);
    Ledger::new(storage).unwrap()
}

fn make_block(tx_count: usize) -> Block {
    let proposer = H160::from_slice(&[0x01; 20]).unwrap();
    let keypairs: Vec<Keypair> = (0..tx_count).map(|_| Keypair::generate()).collect();
    let recipient = H160::from_slice(&[0x42; 20]).unwrap();
    let txs: Vec<Transaction> = keypairs
        .iter()
        .map(|kp| make_transfer_tx(kp, recipient, 100, 0, 100))
        .collect();
    Block::new(1, H256::zero(), proposer, make_vrf_proof(), txs)
}

fn bench_block_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_hash");
    for tx_count in [0, 10, 100, 500] {
        let block = make_block(tx_count);
        group.bench_with_input(BenchmarkId::new("txs", tx_count), &block, |b, block| {
            b.iter(|| black_box(block.hash()))
        });
    }
    group.finish();
}

fn bench_block_serialized_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_serialized_size");
    for tx_count in [0, 10, 100, 500] {
        let block = make_block(tx_count);
        group.bench_with_input(BenchmarkId::new("txs", tx_count), &block, |b, block| {
            b.iter(|| black_box(bincode::serialized_size(block).unwrap()))
        });
    }
    group.finish();
}

fn bench_compute_transactions_root(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_transactions_root");
    for tx_count in [10, 50, 100, 500, 1000] {
        let keypairs: Vec<Keypair> = (0..tx_count).map(|_| Keypair::generate()).collect();
        let txs: Vec<Transaction> = keypairs
            .iter()
            .map(|kp| make_signed_tx(kp, 0, 100))
            .collect();
        group.bench_with_input(BenchmarkId::from_parameter(tx_count), &txs, |b, txs| {
            b.iter(|| black_box(compute_transactions_root(txs)))
        });
    }
    group.finish();
}

fn bench_compute_receipts_root(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_receipts_root");
    for count in [10, 50, 100, 500, 1000] {
        let receipts: Vec<TransactionReceipt> = (0..count).map(make_receipt).collect();
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &receipts,
            |b, receipts| b.iter(|| black_box(compute_receipts_root(receipts))),
        );
    }
    group.finish();
}

fn bench_end_to_end_block_production(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_block_production");
    for tx_count in [10, 50, 100] {
        let senders: Vec<Keypair> = (0..tx_count).map(|_| Keypair::generate()).collect();
        let sender_addrs: Vec<Address> = senders
            .iter()
            .map(|kp| H160::from_slice(&kp.to_address()).unwrap())
            .collect();
        let recipient = H160::from_slice(&[0x99; 20]).unwrap();
        let txs: Vec<Transaction> = senders
            .iter()
            .map(|kp| make_transfer_tx(kp, recipient, 100, 0, 100))
            .collect();

        group.bench_with_input(
            BenchmarkId::new("txs", tx_count),
            &(sender_addrs.clone(), txs.clone()),
            |b, (addrs, txs)| {
                b.iter_with_setup(
                    || {
                        let mut ledger = temp_ledger();
                        for addr in addrs {
                            ledger.seed_account(addr, 1_000_000_000).unwrap();
                        }
                        ledger
                    },
                    |mut ledger| {
                        let (receipts, _overlay) = ledger.apply_block_speculatively(txs).unwrap();
                        let tx_root = compute_transactions_root(txs);
                        let rx_root = compute_receipts_root(&receipts);
                        black_box((tx_root, rx_root));
                    },
                )
            },
        );
    }
    group.finish();
}

fn bench_duplicate_tx_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("duplicate_tx_detection");
    for tx_count in [100, 500, 1000] {
        let keypairs: Vec<Keypair> = (0..tx_count).map(|_| Keypair::generate()).collect();
        let txs: Vec<Transaction> = keypairs
            .iter()
            .map(|kp| make_signed_tx(kp, 0, 100))
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(tx_count), &txs, |b, txs| {
            b.iter(|| {
                let mut seen = HashSet::with_capacity(txs.len());
                for tx in txs {
                    seen.insert(tx.hash());
                }
                black_box(seen.len());
            })
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_block_hash,
    bench_block_serialized_size,
    bench_compute_transactions_root,
    bench_compute_receipts_root,
    bench_end_to_end_block_production,
    bench_duplicate_tx_detection,
);
criterion_main!(benches);
