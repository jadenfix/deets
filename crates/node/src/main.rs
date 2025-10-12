use aether_crypto_primitives::Keypair;
use aether_node::Node;
use aether_types::{Address, PublicKey, Signature, Transaction, ValidatorInfo};
use std::collections::HashSet;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Aether Node v0.1.0");
    println!("==================\n");

    // Generate validator keys
    let validator_key = Keypair::generate();
    let validator_pubkey = PublicKey::from_bytes(validator_key.public_key());

    // Create single validator for testing
    let validators = vec![ValidatorInfo {
        pubkey: validator_pubkey.clone(),
        stake: 1_000_000,
        commission: 1000, // 10%
        active: true,
    }];

    let validator_address =
        Address::from_slice(&validator_key.to_address()).map_err(|e| anyhow::anyhow!(e))?;

    println!("Validator address: {:?}", validator_address);
    println!("Starting node...\n");

    // Create and run node
    let mut node = Node::new("./data/node1", validators, Some(validator_key))?;

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
