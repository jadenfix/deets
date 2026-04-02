use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_consensus::hybrid::HybridConsensus;
use aether_consensus::has_quorum;
use aether_consensus::ConsensusEngine;
use aether_crypto_bls::BlsKeypair;
use aether_crypto_primitives::Keypair;
use aether_types::{PublicKey, Signature, ValidatorInfo, Vote, H256};

fn create_validator_with_bls(stake: u128) -> (ValidatorInfo, BlsKeypair) {
    let keypair = Keypair::generate();
    let bls_kp = BlsKeypair::generate();
    let vi = ValidatorInfo {
        pubkey: PublicKey::from_bytes(keypair.public_key()),
        stake,
        commission: 0,
        active: true,
    };
    (vi, bls_kp)
}

fn make_signed_vote(
    consensus: &mut HybridConsensus,
    vi: &ValidatorInfo,
    bls_kp: &BlsKeypair,
    block_hash: H256,
    slot: u64,
) -> Vote {
    let addr = vi.pubkey.to_address();
    let pop = bls_kp.proof_of_possession();
    let _ = consensus.register_bls_pubkey(addr, bls_kp.public_key(), &pop);
    let mut msg = Vec::new();
    msg.extend_from_slice(block_hash.as_bytes());
    msg.extend_from_slice(&slot.to_le_bytes());
    let sig = bls_kp.sign(&msg);
    Vote {
        slot,
        block_hash,
        validator: vi.pubkey.clone(),
        signature: Signature::from_bytes(sig),
        stake: vi.stake,
    }
}

/// Benchmark BLS key generation
fn bench_bls_keygen(c: &mut Criterion) {
    c.bench_function("bls_keygen", |b| {
        b.iter(|| black_box(BlsKeypair::generate()));
    });
}

/// Benchmark BLS signing (vote message)
fn bench_bls_sign(c: &mut Criterion) {
    let bls_kp = BlsKeypair::generate();
    let block_hash = H256::from_slice(&[0xAB; 32]).unwrap();
    let mut msg = Vec::new();
    msg.extend_from_slice(block_hash.as_bytes());
    msg.extend_from_slice(&0u64.to_le_bytes());

    c.bench_function("bls_sign_vote", |b| {
        b.iter(|| black_box(bls_kp.sign(black_box(&msg))));
    });
}

/// Benchmark BLS signature verification
fn bench_bls_verify(c: &mut Criterion) {
    let bls_kp = BlsKeypair::generate();
    let block_hash = H256::from_slice(&[0xAB; 32]).unwrap();
    let mut msg = Vec::new();
    msg.extend_from_slice(block_hash.as_bytes());
    msg.extend_from_slice(&0u64.to_le_bytes());
    let sig = bls_kp.sign(&msg);
    let pk = bls_kp.public_key();

    c.bench_function("bls_verify_vote", |b| {
        b.iter(|| {
            black_box(
                aether_crypto_bls::keypair::verify(black_box(&pk), black_box(&msg), black_box(&sig))
                    .unwrap(),
            )
        });
    });
}

/// Benchmark BLS signature aggregation at various validator counts
fn bench_bls_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("bls_aggregate_signatures");
    for n in [4, 16, 64, 128] {
        let keypairs: Vec<BlsKeypair> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let block_hash = H256::from_slice(&[0xAB; 32]).unwrap();
        let mut msg = Vec::new();
        msg.extend_from_slice(block_hash.as_bytes());
        msg.extend_from_slice(&0u64.to_le_bytes());
        let sigs: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.sign(&msg)).collect();

        group.bench_with_input(BenchmarkId::new("signatures", n), &sigs, |b, sigs| {
            b.iter(|| black_box(aether_crypto_bls::aggregate_signatures(black_box(sigs)).unwrap()));
        });
    }
    group.finish();

    let mut group = c.benchmark_group("bls_aggregate_pubkeys");
    for n in [4, 16, 64, 128] {
        let keypairs: Vec<BlsKeypair> = (0..n).map(|_| BlsKeypair::generate()).collect();
        let pubkeys: Vec<Vec<u8>> = keypairs.iter().map(|kp| kp.public_key()).collect();

        group.bench_with_input(BenchmarkId::new("pubkeys", n), &pubkeys, |b, pks| {
            b.iter(|| black_box(aether_crypto_bls::aggregate_public_keys(black_box(pks)).unwrap()));
        });
    }
    group.finish();
}

/// Benchmark vote processing (the hot path in consensus)
fn bench_process_vote(c: &mut Criterion) {
    let mut group = c.benchmark_group("process_vote");
    for n in [4, 16, 64] {
        // Setup: create n validators
        let validators_and_keys: Vec<(ValidatorInfo, BlsKeypair)> =
            (0..n).map(|_| create_validator_with_bls(1000)).collect();
        let validators: Vec<ValidatorInfo> = validators_and_keys.iter().map(|(v, _)| v.clone()).collect();

        group.bench_with_input(BenchmarkId::new("validators", n), &n, |b, _| {
            b.iter_batched(
                || {
                    // Setup fresh consensus per iteration
                    let mut consensus =
                        HybridConsensus::new(validators.clone(), 0.8, 100, None, None, None);
                    let block_hash = H256::from_slice(&[0xAB; 32]).unwrap();
                    // Create all signed votes
                    let votes: Vec<Vote> = validators_and_keys
                        .iter()
                        .map(|(vi, bls_kp)| {
                            make_signed_vote(&mut consensus, vi, bls_kp, block_hash, 0)
                        })
                        .collect();
                    (consensus, votes)
                },
                |(mut consensus, votes)| {
                    for vote in votes {
                        let _ = black_box(consensus.process_vote(black_box(vote)));
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Benchmark quorum checking (pure arithmetic)
fn bench_has_quorum(c: &mut Criterion) {
    c.bench_function("has_quorum", |b| {
        b.iter(|| {
            black_box(has_quorum(
                black_box(67_000_000_000),
                black_box(100_000_000_000),
            ))
        });
    });
}

/// Benchmark VRF leader eligibility check
fn bench_vrf_eligibility(c: &mut Criterion) {
    use aether_crypto_vrf::VrfKeypair;

    let vrf_kp = VrfKeypair::generate();
    let validators = vec![ValidatorInfo {
        pubkey: PublicKey::from_bytes(Keypair::generate().public_key()),
        stake: 1000,
        commission: 0,
        active: true,
    }];

    let _consensus = HybridConsensus::new(
        validators,
        0.8,
        100,
        Some(vrf_kp),
        None,
        None,
    );
    let vrf_kp2 = VrfKeypair::generate();
    let slot_msg = format!("slot-eligibility-{}-{}", 42u64, H256::zero());

    c.bench_function("vrf_prove", |b| {
        b.iter(|| black_box(vrf_kp2.prove(black_box(slot_msg.as_bytes()))));
    });
}

/// Benchmark slot advancement
fn bench_advance_slot(c: &mut Criterion) {
    let validators: Vec<ValidatorInfo> = (0..100)
        .map(|_| {
            let kp = Keypair::generate();
            ValidatorInfo {
                pubkey: PublicKey::from_bytes(kp.public_key()),
                stake: 1000,
                commission: 0,
                active: true,
            }
        })
        .collect();

    c.bench_function("advance_slot_100_validators", |b| {
        b.iter_batched(
            || HybridConsensus::new(validators.clone(), 0.8, 100, None, None, None),
            |mut consensus| {
                for _ in 0..100 {
                    consensus.advance_slot();
                }
                black_box(&consensus);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_bls_keygen,
    bench_bls_sign,
    bench_bls_verify,
    bench_bls_aggregation,
    bench_process_vote,
    bench_has_quorum,
    bench_vrf_eligibility,
    bench_advance_slot,
);
criterion_main!(benches);
