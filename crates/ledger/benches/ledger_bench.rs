use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_crypto_primitives::Keypair;
use aether_ledger::Ledger;
use aether_state_storage::Storage;
use aether_types::{
    Address, PublicKey, Signature, Transaction, TransferPayload, H160, TRANSFER_PROGRAM_ID,
};
use std::collections::HashSet;

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

fn temp_ledger() -> Ledger {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let storage = Storage::open(&path).unwrap();
    // Leak the TempDir so it isn't deleted while the benchmark runs
    std::mem::forget(dir);
    Ledger::new(storage).unwrap()
}

fn fund_account(ledger: &mut Ledger, address: &Address, balance: u128) {
    ledger.seed_account(address, balance).unwrap();
}

fn bench_signature_verification(c: &mut Criterion) {
    let keypair = Keypair::generate();
    let tx = make_signed_tx(&keypair, 0, 100);

    c.bench_function("tx_signature_verify", |b| {
        b.iter(|| black_box(&tx).verify_signature().unwrap())
    });
}

fn bench_tx_hash(c: &mut Criterion) {
    let keypair = Keypair::generate();
    let tx = make_signed_tx(&keypair, 0, 100);

    c.bench_function("tx_hash", |b| {
        b.iter(|| black_box(&tx).hash())
    });
}

fn bench_apply_simple_tx(c: &mut Criterion) {
    let keypair = Keypair::generate();
    let address = H160::from_slice(&keypair.to_address()).unwrap();

    c.bench_function("apply_simple_tx", |b| {
        b.iter_with_setup(
            || {
                let mut ledger = temp_ledger();
                fund_account(&mut ledger, &address, 1_000_000_000);
                let tx = make_signed_tx(&keypair, 0, 100);
                (ledger, tx)
            },
            |(mut ledger, tx)| {
                black_box(ledger.apply_transaction(&tx).unwrap());
            },
        )
    });
}

fn bench_apply_transfer_tx(c: &mut Criterion) {
    let sender_kp = Keypair::generate();
    let sender_addr = H160::from_slice(&sender_kp.to_address()).unwrap();
    let recipient = H160::from_slice(&[42u8; 20]).unwrap();

    c.bench_function("apply_transfer_tx", |b| {
        b.iter_with_setup(
            || {
                let mut ledger = temp_ledger();
                fund_account(&mut ledger, &sender_addr, 1_000_000_000);
                let tx = make_transfer_tx(&sender_kp, recipient, 1000, 0, 100);
                (ledger, tx)
            },
            |(mut ledger, tx)| {
                black_box(ledger.apply_transaction(&tx).unwrap());
            },
        )
    });
}

fn bench_sequential_transactions(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_txs");
    for count in [10, 50, 100] {
        let keypair = Keypair::generate();
        let address = H160::from_slice(&keypair.to_address()).unwrap();
        let recipient = H160::from_slice(&[42u8; 20]).unwrap();

        let txs: Vec<Transaction> = (0..count)
            .map(|i| make_transfer_tx(&keypair, recipient, 100, i as u64, 100))
            .collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &txs,
            |b, txs| {
                b.iter_with_setup(
                    || {
                        let mut ledger = temp_ledger();
                        fund_account(&mut ledger, &address, 1_000_000_000);
                        ledger
                    },
                    |mut ledger| {
                        for tx in txs {
                            black_box(ledger.apply_transaction(tx).unwrap());
                        }
                    },
                )
            },
        );
    }
    group.finish();
}

fn bench_state_root_computation(c: &mut Criterion) {
    let keypair = Keypair::generate();
    let address = H160::from_slice(&keypair.to_address()).unwrap();

    c.bench_function("state_root_after_tx", |b| {
        b.iter_with_setup(
            || {
                let mut ledger = temp_ledger();
                fund_account(&mut ledger, &address, 1_000_000_000);
                let tx = make_signed_tx(&keypair, 0, 100);
                (ledger, tx)
            },
            |(mut ledger, tx)| {
                ledger.apply_transaction(&tx).unwrap();
                black_box(ledger.state_root());
            },
        )
    });
}

criterion_group!(
    benches,
    bench_signature_verification,
    bench_tx_hash,
    bench_apply_simple_tx,
    bench_apply_transfer_tx,
    bench_sequential_transactions,
    bench_state_root_computation,
);
criterion_main!(benches);
