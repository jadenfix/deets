use aether_crypto_primitives::Keypair;
use aether_node::{create_hybrid_consensus, validator_info_from_keypair, Node, ValidatorKeypair};
use aether_types::{Address, PublicKey, Signature, Transaction};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Aether Node v0.1.0 - Phase 1 Integration");
    println!("=========================================\n");

    // Generate validator keys (Ed25519 + VRF + BLS)
    let validator_keypair = ValidatorKeypair::generate();
    let validator_pubkey = validator_keypair.public_key();
    let validator_address = validator_keypair.address();

    // Create validator info
    let validators = vec![validator_info_from_keypair(&validator_keypair, 1_000_000)];

    println!("Validator address: {:?}", validator_address);
    println!("Consensus: VRF + HotStuff + BLS");
    println!("Starting node...\n");

    // Create hybrid consensus engine (VRF + HotStuff + BLS)
    let consensus = Box::new(create_hybrid_consensus(
        validators,
        Some(&validator_keypair),
        0.8,   // tau: 80% leader rate
        100,   // epoch length: 100 slots
    )?);

    // Create and run node
    let mut node = Node::new(
        "./data/node1",
        consensus,
        Some(validator_keypair.ed25519),
    )?;

    // Add a test transaction
    let test_tx = Transaction {
        nonce: 0,
        sender: Address::from_slice(&[1u8; 20]).map_err(|e| anyhow::anyhow!(e))?,
        sender_pubkey: validator_pubkey.clone(),
        inputs: vec![],
        outputs: vec![],
        reads: HashSet::new(),
        writes: HashSet::new(),
        program_id: None,
        data: vec![],
        gas_limit: 21000,
        fee: 5000,
        signature: Signature::from_bytes(vec![]),
    };

    println!("Submitting test transaction...");
    let tx_hash = node.submit_transaction(test_tx)?;
    println!("Transaction submitted: {}\n", tx_hash);

    // Run for a few slots
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        println!("\nShutting down...");
        std::process::exit(0);
    });

    node.run().await?;

    Ok(())
}
