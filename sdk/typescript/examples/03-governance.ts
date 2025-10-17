/**
 * Example 3: Governance Participation
 * 
 * Demonstrates:
 * - Viewing active proposals
 * - Creating a proposal
 * - Voting on proposals
 * - Checking proposal status
 */

import { AetherClient, AetherKeypair, GovernanceHelper } from '@aether/sdk';

async function main() {
  const client = new AetherClient({ rpcUrl: 'http://localhost:8545' });
  const keypair = await AetherKeypair.fromSeed('my seed phrase');

  const gov = new GovernanceHelper(client, keypair);

  console.log('Fetching active proposals...');
  const proposals = await gov.getActiveProposals();
  
  console.log(`Found ${proposals.length} active proposals\n`);
  
  for (const p of proposals) {
    console.log(`Proposal #${p.id}: ${p.title}`);
    console.log(`- Status: ${p.status}`);
    console.log(`- Votes For: ${p.votesFor}`);
    console.log(`- Votes Against: ${p.votesAgainst}`);
    
    const status = await gov.getProposalStatus(p.id);
    if (status) {
      console.log(`- Has Quorum: ${status.hasQuorum}`);
      console.log(`- Time Remaining: ${status.timeRemaining} slots`);
    }
    console.log();
  }

  console.log('Creating a new proposal...');
  const createTx = await gov.createProposal(
    'Increase Validator Rewards',
    'Proposal to increase validator block rewards from 10 AIC to 15 AIC to improve network security',
    100800
  );
  
  const createHash = await client.sendTransaction(createTx);
  console.log('Proposal created:', createHash);
  await client.waitForTransaction(createHash);

  if (proposals.length > 0) {
    const proposalId = proposals[0].id;
    console.log(`\nVoting on proposal #${proposalId}...`);
    
    const votingPower = await gov.getVotingPower(keypair.address);
    console.log('Your voting power:', votingPower.toString());
    
    const voteTx = await gov.vote(proposalId, true);
    const voteHash = await client.sendTransaction(voteTx);
    console.log('Vote submitted:', voteHash);
    await client.waitForTransaction(voteHash);
    console.log('Vote confirmed');

    const myVote = await gov.getVote(proposalId, keypair.address);
    console.log('\nYour vote:');
    console.log('- Support:', myVote?.support);
    console.log('- Power:', myVote?.votingPower.toString());
  }
}

main().catch(console.error);

