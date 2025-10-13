--------------------------- MODULE HotStuffVRF ---------------------------
(***************************************************************************
 * TLA+ Specification of Aether's VRF-PoS + HotStuff Consensus
 *
 * This specification models the safety and liveness properties of the
 * consensus protocol used in the Aether blockchain.
 *
 * Key Components:
 * - VRF-based leader election (Ouroboros-style)
 * - HotStuff 2-chain BFT consensus
 * - BLS aggregate vote signatures
 * - Byzantine fault tolerance (f < n/3)
 *
 * Safety Property: No two conflicting blocks are finalized
 * Liveness Property: Eventually, blocks are finalized under synchrony
 ***************************************************************************)

EXTENDS Integers, Sequences, FiniteSets, TLC

CONSTANTS
    Validators,      \* Set of validator identities
    MaxSlot,        \* Maximum slot number to model-check
    ByzantineQuota  \* Maximum Byzantine validators (< |Validators|/3)

VARIABLES
    slot,           \* Current slot number
    blocks,         \* Set of all proposed blocks  
    votes,          \* Votes cast by validators
    finalized,      \* Set of finalized blocks
    leader,         \* Current slot leader (VRF winner)
    msgs            \* Network messages in flight

vars == <<slot, blocks, votes, finalized, leader, msgs>>

-----------------------------------------------------------------------------
(* Type Invariants *)

Block == [
    slot: Nat,
    parent: Nat \union {0},  \* 0 = genesis
    proposer: Validators,
    vrfProof: STRING
]

Vote == [
    block: Nat,              \* Block hash (ID)
    validator: Validators,
    signature: STRING
]

Message == [
    type: {"PROPOSE", "VOTE", "FINALIZE"},
    block: Nat,
    sender: Validators
]

TypeOK ==
    /\ slot \in Nat
    /\ blocks \subseteq Block
    /\ votes \subseteq Vote
    /\ finalized \subseteq blocks
    /\ leader \in Validators \union {NULL}
    /\ msgs \subseteq Message

-----------------------------------------------------------------------------
(* Helper Functions *)

\* Quorum is 2/3 + 1 of validators
Quorum == (2 * Cardinality(Validators)) \div 3 + 1

\* Count votes for a specific block
VotesForBlock(b) ==
    Cardinality({v \in votes : v.block = b})

\* Check if block has quorum
HasQuorum(b) ==
    VotesForBlock(b) >= Quorum

\* Get parent block
Parent(b) ==
    IF b.parent = 0 
    THEN NULL
    ELSE CHOOSE p \in blocks : p.slot = b.parent

\* Check if block extends chain (no conflicts)
ExtendsChain(b) ==
    \/ b.parent = 0  \* Genesis
    \/ \E p \in blocks : p.slot = b.parent

\* Byzantine validators (model non-deterministically)
ByzantineValidators ==
    CHOOSE S \in SUBSET Validators :
        Cardinality(S) <= ByzantineQuota

\* Honest validators
HonestValidators ==
    Validators \ ByzantineValidators

-----------------------------------------------------------------------------
(* Initial State *)

Init ==
    /\ slot = 0
    /\ blocks = {}
    /\ votes = {}
    /\ finalized = {}
    /\ leader = NULL
    /\ msgs = {}

-----------------------------------------------------------------------------
(* VRF Leader Election *)

\* VRF election selects leader based on stake + randomness
\* Simplified: deterministic round-robin for model checking
ElectLeader ==
    /\ slot' = slot + 1
    /\ leader' \in Validators  \* Non-deterministic choice (models VRF)
    /\ UNCHANGED <<blocks, votes, finalized, msgs>>

-----------------------------------------------------------------------------
(* Block Proposal *)

ProposeBlock ==
    /\ leader # NULL
    /\ \E parent \in (blocks \union {NULL}):
        LET newBlock == [
            slot |-> slot,
            parent |-> IF parent = NULL THEN 0 ELSE parent.slot,
            proposer |-> leader,
            vrfProof |-> "vrf_proof"
        ]
        IN
            /\ ExtendsChain(newBlock)
            /\ blocks' = blocks \union {newBlock}
            /\ msgs' = msgs \union {[
                type |-> "PROPOSE",
                block |-> newBlock.slot,
                sender |-> leader
            ]}
            /\ UNCHANGED <<slot, votes, finalized, leader>>

-----------------------------------------------------------------------------
(* Voting *)

\* Honest validator votes for valid block
CastVote(v) ==
    /\ v \in HonestValidators
    /\ \E b \in blocks:
        /\ b.slot = slot
        /\ ExtendsChain(b)
        /\ ~\E oldVote \in votes : 
            /\ oldVote.validator = v 
            /\ oldVote.block = b.slot
        /\ LET newVote == [
            block |-> b.slot,
            validator |-> v,
            signature |-> "bls_sig"
        ]
        IN
            /\ votes' = votes \union {newVote}
            /\ msgs' = msgs \union {[
                type |-> "VOTE",
                block |-> b.slot,
                sender |-> v
            ]}
            /\ UNCHANGED <<slot, blocks, finalized, leader>>

\* Byzantine validator may vote maliciously (model checking explores this)
ByzantineVote(v) ==
    /\ v \in ByzantineValidators
    /\ \E b1, b2 \in blocks:
        /\ b1 # b2
        /\ b1.slot = b2.slot  \* Equivocation
        /\ LET 
            vote1 == [block |-> b1.slot, validator |-> v, signature |-> "bls_1"]
            vote2 == [block |-> b2.slot, validator |-> v, signature |-> "bls_2"]
        IN
            /\ votes' = votes \union {vote1, vote2}
            /\ UNCHANGED <<slot, blocks, finalized, leader, msgs>>

-----------------------------------------------------------------------------
(* Finalization (HotStuff 2-chain rule) *)

\* A block is finalized when it has a quorum and its child has a quorum
FinalizeBlock ==
    /\ \E b \in blocks:
        /\ HasQuorum(b.slot)
        /\ \E child \in blocks:
            /\ child.parent = b.slot
            /\ HasQuorum(child.slot)
        /\ b \notin finalized
        /\ finalized' = finalized \union {b}
        /\ msgs' = msgs \union {[
            type |-> "FINALIZE",
            block |-> b.slot,
            sender |-> leader
        ]}
        /\ UNCHANGED <<slot, blocks, votes, leader>>

-----------------------------------------------------------------------------
(* Next State Transition *)

Next ==
    \/ ElectLeader
    \/ ProposeBlock
    \/ \E v \in HonestValidators : CastVote(v)
    \/ \E v \in ByzantineValidators : ByzantineVote(v)
    \/ FinalizeBlock

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

-----------------------------------------------------------------------------
(* Safety Properties *)

\* PROPERTY: No two conflicting blocks are finalized
ConflictingBlocks(b1, b2) ==
    /\ b1 # b2
    /\ b1.slot = b2.slot
    /\ b1.parent # b2.parent

Safety ==
    ~\E b1, b2 \in finalized : ConflictingBlocks(b1, b2)

\* PROPERTY: Finalized blocks form a chain
FinalizedChainProperty ==
    \A b \in finalized :
        b.parent = 0 \/ \E p \in finalized : p.slot = b.parent

\* PROPERTY: Once finalized, block is never reverted
MonotonicFinality ==
    [][\A b \in finalized : b \in finalized']_finalized

-----------------------------------------------------------------------------
(* Liveness Properties *)

\* Under partial synchrony, blocks are eventually finalized
\* (requires fairness assumptions on leader election and message delivery)

EventuallyFinalized ==
    \A b \in blocks : <>(b \in finalized \/ ~ExtendsChain(b))

\* Progress: slot number increases
Progress ==
    <>[](slot > 0)

-----------------------------------------------------------------------------
(* Model Checking Configuration *)

\* State constraint to limit model checking
StateConstraint ==
    /\ slot <= MaxSlot
    /\ Cardinality(blocks) <= 20
    /\ Cardinality(votes) <= 60

\* Symmetry reduction (validators are interchangeable)
Symmetry == Permutations(Validators)

-----------------------------------------------------------------------------
(* Theorems to Check *)

THEOREM Safety  \* No conflicting finalized blocks
THEOREM FinalizedChainProperty  \* Finalized blocks form chain
THEOREM MonotonicFinality  \* Finality is irreversible

=============================================================================

(***************************************************************************
 * Model Checking Instructions:
 *
 * 1. Install TLA+ Toolbox: https://github.com/tlaplus/tlaplus/releases
 * 
 * 2. Create a model with these constants:
 *    - Validators = {v1, v2, v3, v4}
 *    - MaxSlot = 5
 *    - ByzantineQuota = 1
 *
 * 3. Add invariants to check:
 *    - TypeOK
 *    - Safety
 *    - FinalizedChainProperty
 *
 * 4. Add temporal properties:
 *    - MonotonicFinality
 *    - EventuallyFinalized (with fairness)
 *
 * 5. Run model checker (TLC):
 *    - Depth-first search
 *    - State constraint: StateConstraint
 *    - Symmetry: Symmetry
 *
 * Expected Results:
 * - Safety should hold under all behaviors
 * - Liveness requires fairness assumptions
 * - Byzantine quota < n/3 ensures safety
 *
 ***************************************************************************)

