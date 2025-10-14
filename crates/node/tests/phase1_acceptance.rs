use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use aether_consensus::SimpleConsensus;
use aether_crypto_bls::aggregate::{aggregate_public_keys, aggregate_signatures};
use aether_crypto_bls::{verify_aggregated, BlsKeypair};
use aether_crypto_primitives::Keypair;
use aether_crypto_vrf::{check_leader_eligibility, verify_proof, VrfKeypair};
use aether_quic_transport::{connection::QuicConnection, QuicEndpoint};
use aether_runtime::{ExecutionContext, ParallelScheduler, WasmVm};
use aether_types::{
    Address, PublicKey, Signature, Slot, Transaction, UtxoOutput, ValidatorInfo, Vote, H160, H256,
};
use tokio::sync::Mutex;

#[test]
fn phase1_ecvrf_leader_election() {
    let vrf = VrfKeypair::generate();
    let slot: Slot = 42;
    let mut input = Vec::new();
    input.extend_from_slice(&slot.to_le_bytes());

    let proof = vrf.prove(&input);
    assert_eq!(proof.output.len(), 32);

    let verified = verify_proof(vrf.public_key(), &input, &proof).expect("VRF verification");
    assert!(verified, "VRF proof should verify for the matching key");

    let total_stake = 10_000u128;
    let validator_stake = total_stake;
    assert!(
        check_leader_eligibility(&proof.output, validator_stake, total_stake, 1.0),
        "full-stake validator with tau=1.0 must always be eligible"
    );

    assert!(
        !check_leader_eligibility(&proof.output, 0, total_stake, 0.0),
        "zero stake validator should never win the lottery"
    );
}

#[test]
fn phase1_bls_vote_aggregation() {
    let message = b"phase1-block-hash";

    let mut signatures = Vec::new();
    let mut public_keys = Vec::new();

    for _ in 0..4 {
        let keypair = BlsKeypair::generate();
        signatures.push(keypair.sign(message));
        public_keys.push(keypair.public_key());
    }

    let aggregated_sig = aggregate_signatures(&signatures).expect("aggregate signatures");
    let aggregated_pk = aggregate_public_keys(&public_keys).expect("aggregate pubkeys");

    let verified =
        verify_aggregated(&aggregated_pk, message, &aggregated_sig).expect("verify aggregated");
    assert!(
        verified,
        "aggregated BLS signature should verify for the combined key"
    );
}

#[test]
fn phase1_simple_consensus_finality() {
    let validators: Vec<(Keypair, ValidatorInfo)> = (0..4)
        .map(|_| {
            let kp = Keypair::generate();
            let info = ValidatorInfo {
                pubkey: PublicKey::from_bytes(kp.public_key()),
                stake: 1_000,
                commission: 0,
                active: true,
            };
            (kp, info)
        })
        .collect();

    let validator_infos: Vec<ValidatorInfo> =
        validators.iter().map(|(_, info)| info.clone()).collect();
    let mut consensus = SimpleConsensus::new(validator_infos.clone());

    consensus.advance_slot();
    let slot = consensus.current_slot();

    for (_, info) in validators.iter().take(3) {
        let vote = Vote {
            slot,
            block_hash: H256::zero(),
            validator: info.pubkey.clone(),
            signature: Signature::from_bytes(vec![0; 64]),
            stake: info.stake,
        };
        consensus.add_vote(vote).expect("add vote");
    }

    assert!(
        consensus.check_finality(slot),
        "finality should trigger with votes representing ≥2/3 stake"
    );
    assert_eq!(consensus.finalized_slot(), slot);
}

#[test]
fn phase1_wasm_runtime_executes_minimal_contract() {
    let wasm = b"\0asm\x01\x00\x00\x00";
    let context = ExecutionContext {
        contract_address: Address::from_slice(&[1u8; 20]).unwrap(),
        caller: Address::from_slice(&[2u8; 20]).unwrap(),
        value: 0,
        gas_limit: 50_000,
        block_number: 1,
        timestamp: 1_000,
    };

    let mut vm = WasmVm::new(10_000);
    let result = vm
        .execute(wasm, &context, b"input")
        .expect("WASM execution succeeds");

    assert!(result.success);
    assert!(result.gas_used > 0);
}

#[test]
fn phase1_parallel_scheduler_speedup() {
    let scheduler = ParallelScheduler::new();
    let txs: Vec<Transaction> = (0..12).map(create_synthetic_tx).collect();

    let batches = scheduler.schedule(&txs);
    assert!(
        !batches.is_empty(),
        "scheduler must produce at least one batch"
    );

    let speedup = scheduler.speedup_estimate(&txs);
    assert!(
        speedup > 1.0,
        "independent transactions should yield parallel speedup, got {speedup}"
    );
}

#[tokio::test]
async fn phase1_basic_p2p_networking_quic() {
    let endpoint = QuicEndpoint::new("127.0.0.1:0".parse().unwrap())
        .await
        .expect("create endpoint");
    let addr = endpoint.local_addr().expect("local address");

    let received = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let receiver_endpoint = endpoint.clone();
    let received_clone = received.clone();

    tokio::spawn(async move {
        if let Some(conn) = receiver_endpoint.accept().await {
            let mut stream = conn.accept_uni().await.expect("accept uni stream");
            let data = QuicConnection::read_stream(&mut stream)
                .await
                .expect("read stream");
            received_clone.lock().await.push(data);
        }
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    let payload = b"phase1-quic".to_vec();
    let conn = endpoint.connect(addr).await.expect("connect to self");
    conn.send(payload.clone()).await.expect("send payload");

    tokio::time::sleep(Duration::from_millis(50)).await;

    let stored = received.lock().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0], payload);
}

fn create_synthetic_tx(index: u8) -> Transaction {
    let address = Address::from_slice(&[index; 20]).unwrap();
    let pubkey = PublicKey::from_bytes(vec![index; 32]);

    let mut writes = HashSet::new();
    writes.insert(address);

    Transaction {
        nonce: 0,
        sender: H160::from_slice(&[index; 20]).unwrap(),
        sender_pubkey: pubkey,
        inputs: vec![],
        outputs: vec![UtxoOutput {
            amount: 1_000,
            owner: PublicKey::from_bytes(vec![index; 32]),
            script_hash: None,
        }],
        reads: HashSet::new(),
        writes,
        program_id: None,
        data: vec![],
        gas_limit: 21_000,
        fee: 1_000_000,
        signature: Signature::from_bytes(vec![0; 64]),
    }
}
