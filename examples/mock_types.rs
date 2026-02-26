// Mock Cardano blockchain types for wallet simulation
// These represent simplified versions of real Cardano structures

use serde::{Serialize, Deserialize};

/// A simplified Cardano block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub height: u64,
    pub hash: BlockHash,
    pub slot: u64,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockHash(pub String);

/// A simplified Cardano transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub hash: TxHash,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub fee: u64,
    pub metadata: Option<Vec<u8>>,
    pub governance_actions: Vec<GovernanceAction>,
    pub votes: Vec<Vote>,
    pub certificates: Vec<Certificate>,
    pub withdrawals: Vec<(StakeKey, u64)>, // Withdraw rewards
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxHash(pub String);

impl TxHash {
    pub fn new(s: impl Into<String>) -> Self {
        TxHash(s.into())
    }
}

/// Transaction input (spending a UTXO)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    pub tx_hash: TxHash,
    pub output_index: u32,
}

/// Transaction output (creating a UTXO)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    pub address: Address,
    pub amount: u64,
    pub assets: Vec<Asset>,
    pub datum: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(pub String);

impl Address {
    pub fn new(s: impl Into<String>) -> Self {
        Address(s.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub policy_id: String,
    pub asset_name: String,
    pub amount: u64,
}

/// UTXO reference
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UtxoRef {
    pub tx_hash: TxHash,
    pub output_index: u32,
}

impl UtxoRef {
    pub fn to_key(&self) -> String {
        format!("{}#{}", self.tx_hash.0, self.output_index)
    }
}

/// Full UTXO with its data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utxo {
    pub tx_hash: TxHash,
    pub output_index: u32,
    pub address: Address,
    pub amount: u64,
    pub assets: Vec<Asset>,
    pub datum: Option<Vec<u8>>,
    pub created_at_block: u64,
}

impl Utxo {
    pub fn reference(&self) -> UtxoRef {
        UtxoRef {
            tx_hash: self.tx_hash.clone(),
            output_index: self.output_index,
        }
    }
    
    pub fn to_key(&self) -> String {
        self.reference().to_key()
    }
}

/// Governance proposal/action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceAction {
    pub action_id: ActionId,
    pub action_type: GovernanceActionType,
    pub proposed_in_epoch: u64,
    pub expires_in_epoch: u64,
    pub deposit: u64,
    pub return_address: Address,
    pub description: String,
    pub status: ProposalStatus,
    pub enacted_epoch: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub String);

impl ActionId {
    pub fn new(s: impl Into<String>) -> Self {
        ActionId(s.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceActionType {
    ParameterChange { parameters: HashMap<String, String> },
    HardFork { new_version: u32 },
    TreasuryWithdrawal { amount: u64, recipient: Address },
    NoConfidence,
    UpdateCommittee { changes: String },
    NewConstitution { hash: String },
    Info { note: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    Active,       // Currently voting
    Passed,       // Met threshold, pending enactment
    Failed,       // Did not meet threshold
    Expired,      // Voting period ended
    Enacted,      // Enacted on-chain
}

/// A vote on a governance action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub action_id: ActionId,
    pub voter: Voter,
    pub vote: VoteChoice,
    pub epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Voter {
    DRep(DRepId),
    StakePoolOperator(PoolId),
    ConstitutionalCommittee(CommitteeId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DRepId(pub String);

impl DRepId {
    pub fn new(s: impl Into<String>) -> Self {
        DRepId(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PoolId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommitteeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

/// Voting statistics for a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingStats {
    pub action_id: ActionId,
    pub total_yes_stake: u64,
    pub total_no_stake: u64,
    pub total_abstain_stake: u64,
    pub drep_threshold: u64,
    pub spo_threshold: u64,
    pub cc_threshold: u64,
    pub current_epoch: u64,
    pub expires_epoch: u64,
}

impl VotingStats {
    pub fn has_passed_drep_threshold(&self) -> bool {
        self.total_yes_stake >= self.drep_threshold
    }
    
    pub fn has_passed_spo_threshold(&self) -> bool {
        self.total_yes_stake >= self.spo_threshold
    }
    
    pub fn has_passed_cc_threshold(&self) -> bool {
        self.total_yes_stake >= self.cc_threshold
    }
    
    pub fn has_passed(&self) -> bool {
        self.has_passed_drep_threshold() 
            && self.has_passed_spo_threshold() 
            && self.has_passed_cc_threshold()
    }
    
    pub fn is_active(&self) -> bool {
        self.current_epoch < self.expires_epoch
    }
    
    pub fn days_until_expiry(&self) -> u64 {
        if self.current_epoch < self.expires_epoch {
            (self.expires_epoch - self.current_epoch) * 5 // ~5 days per epoch
        } else {
            0
        }
    }
}

/// Historical voting record (for Merkle tree)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingRecord {
    pub action_id: ActionId,
    pub all_votes: Vec<Vote>,
    pub final_result: ProposalStatus,
    pub enacted_epoch: Option<u64>,
    pub merkle_root: Vec<u8>,
}

/// Stake pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePool {
    pub pool_id: PoolId,
    pub ticker: String,
    pub name: String,
    pub margin: f64,      // Pool margin (e.g., 0.02 = 2%)
    pub fixed_cost: u64,  // Fixed cost per epoch (e.g., 340 ADA)
    pub pledge: u64,      // Pool pledge
    pub active_stake: u64, // Total stake delegated to pool
}

/// Stake key (controls delegation)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StakeKey(pub String);

impl StakeKey {
    pub fn new(s: impl Into<String>) -> Self {
        StakeKey(s.into())
    }
}

/// Stake delegation certificate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationCertificate {
    pub stake_key: StakeKey,
    pub pool_id: PoolId,
    pub epoch: u64,
}

/// Stake registration certificate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeRegistration {
    pub stake_key: StakeKey,
    pub deposit: u64,  // 2 ADA deposit
    pub epoch: u64,
}

/// Rewards distribution for an epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochRewards {
    pub epoch: u64,
    pub stake_key: StakeKey,
    pub rewards: u64,
    pub pool_id: PoolId,
    pub pool_performance: f64, // 0.0 to 1.0
}

/// Current staking status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingStatus {
    pub stake_key: StakeKey,
    pub delegated_pool: Option<PoolId>,
    pub total_rewards: u64,
    pub last_reward_epoch: u64,
    pub is_registered: bool,
}

/// Certificate in transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Certificate {
    StakeRegistration(StakeRegistration),
    StakeDeregistration { stake_key: StakeKey, epoch: u64 },
    Delegation(DelegationCertificate),
    PoolRegistration(StakePool),
    PoolRetirement { pool_id: PoolId, retire_epoch: u64 },
}

/// Mock chain sync client that provides blocks
pub struct MockChainSync {
    blocks: Vec<Block>,
    current_index: usize,
}

impl MockChainSync {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            current_index: 0,
        }
    }
    
    /// Get next block (simulates chain sync)
    pub fn next_block(&mut self) -> Option<Block> {
        if self.current_index < self.blocks.len() {
            let block = self.blocks[self.current_index].clone();
            self.current_index += 1;
            Some(block)
        } else {
            None
        }
    }
    
    /// Get current tip
    pub fn tip(&self) -> Option<u64> {
        self.blocks.last().map(|b| b.height)
    }
    
    /// Simulate a chain reorganization
    pub fn rollback_to(&mut self, height: u64) -> Vec<Block> {
        let rollback_index = self.blocks.iter()
            .position(|b| b.height == height)
            .unwrap_or(0);
        
        let rolled_back: Vec<_> = self.blocks.drain(rollback_index + 1..).collect();
        self.current_index = rollback_index + 1;
        rolled_back
    }
}

/// Helper to generate mock blocks for testing
pub fn generate_mock_blocks(num_blocks: usize, txs_per_block: usize) -> Vec<Block> {
    let mut blocks = Vec::new();
    
    let addresses = vec![
        Address::new("addr1_alice"),
        Address::new("addr1_bob"),
        Address::new("addr1_charlie"),
        Address::new("addr1_dave"),
        Address::new("addr1_eve"),
    ];
    
    for height in 0..num_blocks {
        let mut transactions = Vec::new();
        
        for tx_idx in 0..txs_per_block {
            let tx_hash = TxHash::new(format!("tx_{}_{}", height, tx_idx));
            
            // Some inputs (spending previous UTXOs)
            let inputs = if height > 0 {
                vec![
                    TxInput {
                        tx_hash: TxHash::new(format!("tx_{}_{}", height - 1, tx_idx)),
                        output_index: 0,
                    }
                ]
            } else {
                vec![] // Genesis block has no inputs
            };
            
            // Some outputs (creating new UTXOs)
            let outputs = vec![
                TxOutput {
                    address: addresses[tx_idx % addresses.len()].clone(),
                    amount: 1_000_000 + (tx_idx as u64 * 100_000),
                    assets: vec![],
                    datum: None,
                },
                TxOutput {
                    address: addresses[(tx_idx + 1) % addresses.len()].clone(),
                    amount: 500_000,
                    assets: vec![],
                    datum: None,
                },
            ];
            
            transactions.push(Transaction {
                hash: tx_hash,
                inputs,
                outputs,
                fee: 170_000, // ~0.17 ADA
                metadata: None,
                governance_actions: vec![],
                votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                certificates: vec![],
                withdrawals: vec![],
            });
        }
        
        blocks.push(Block {
            height: height as u64,
            hash: BlockHash(format!("block_{}", height)),
            slot: height as u64 * 20, // ~20 second slots
            transactions,
        });
    }
    
    blocks
}

/// Generate blocks with specific patterns for testing
pub fn generate_test_scenario_blocks() -> Vec<Block> {
    let alice = Address::new("addr1_alice");
    let bob = Address::new("addr1_bob");
    
    vec![
        // Block 0: Alice receives 10 ADA
        Block {
            height: 0,
            hash: BlockHash("block_0".to_string()),
            slot: 0,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_0_0"),
                    inputs: vec![],
                    outputs: vec![
                        TxOutput {
                            address: alice.clone(),
                            amount: 10_000_000,
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 0,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 1: Alice sends 5 ADA to Bob
        Block {
            height: 1,
            hash: BlockHash("block_1".to_string()),
            slot: 20,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_1_0"),
                    inputs: vec![
                        TxInput {
                            tx_hash: TxHash::new("tx_0_0"),
                            output_index: 0,
                        },
                    ],
                    outputs: vec![
                        TxOutput {
                            address: bob.clone(),
                            amount: 5_000_000,
                            assets: vec![],
                            datum: None,
                        },
                        TxOutput {
                            address: alice.clone(),
                            amount: 4_830_000, // 10 - 5 - 0.17 (fee)
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 2: Bob sends 2 ADA to Alice
        Block {
            height: 2,
            hash: BlockHash("block_2".to_string()),
            slot: 40,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_2_0"),
                    inputs: vec![
                        TxInput {
                            tx_hash: TxHash::new("tx_1_0"),
                            output_index: 0,
                        },
                    ],
                    outputs: vec![
                        TxOutput {
                            address: alice.clone(),
                            amount: 2_000_000,
                            assets: vec![],
                            datum: None,
                        },
                        TxOutput {
                            address: bob.clone(),
                            amount: 2_830_000, // 5 - 2 - 0.17
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
    ]
}

/// Generate blocks with governance actions and voting
pub fn generate_governance_scenario_blocks() -> Vec<Block> {
    let alice = Address::new("addr1_alice");
    let bob = Address::new("addr1_bob");
    let charlie = Address::new("addr1_charlie");
    
    let drep_alice = DRepId::new("drep1_alice");
    let drep_bob = DRepId::new("drep1_bob");
    let drep_charlie = DRepId::new("drep1_charlie");
    
    let pool1 = PoolId("pool1".to_string());
    let pool2 = PoolId("pool2".to_string());
    
    vec![
        // Block 0 (Epoch 100): Proposal submitted - Increase k parameter
        Block {
            height: 0,
            hash: BlockHash("block_gov_0".to_string()),
            slot: 0,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_gov_0"),
                    inputs: vec![],
                    outputs: vec![
                        TxOutput {
                            address: alice.clone(),
                            amount: 10_000_000,
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 0,
                    metadata: None,
                    governance_actions: vec![
                        GovernanceAction {
                            action_id: ActionId::new("action_k_param_500"),
                            action_type: GovernanceActionType::ParameterChange {
                                parameters: {
                                    let mut params = HashMap::new();
                                    params.insert("k".to_string(), "500".to_string());
                                    params
                                },
                            },
                            proposed_in_epoch: 100,
                            expires_in_epoch: 106, // 6 epochs to vote
                            deposit: 100_000_000_000, // 100,000 ADA deposit
                            return_address: alice.clone(),
                            description: "Increase k parameter to 500 for better decentralization".to_string(),
                            status: ProposalStatus::Active,
                            enacted_epoch: None,
                        },
                    ],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 1 (Epoch 101): DReps start voting
        Block {
            height: 1,
            hash: BlockHash("block_gov_1".to_string()),
            slot: 432_000, // Next epoch
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_gov_1"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![
                        Vote {
                            action_id: ActionId::new("action_k_param_500"),
                            voter: Voter::DRep(drep_alice.clone()),
                            vote: VoteChoice::Yes,
                            epoch: 101,
                        },
                        Vote {
                            action_id: ActionId::new("action_k_param_500"),
                            voter: Voter::DRep(drep_bob.clone()),
                            vote: VoteChoice::Yes,
                            epoch: 101,
                        },
                    ],
                },
            ],
        },
        
        // Block 2 (Epoch 102): More voting
        Block {
            height: 2,
            hash: BlockHash("block_gov_2".to_string()),
            slot: 864_000,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_gov_2"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![
                        Vote {
                            action_id: ActionId::new("action_k_param_500"),
                            voter: Voter::DRep(drep_charlie.clone()),
                            vote: VoteChoice::No,
                            epoch: 102,
                        },
                        Vote {
                            action_id: ActionId::new("action_k_param_500"),
                            voter: Voter::StakePoolOperator(pool1.clone()),
                            vote: VoteChoice::Yes,
                            epoch: 102,
                        },
                        Vote {
                            action_id: ActionId::new("action_k_param_500"),
                            voter: Voter::StakePoolOperator(pool2.clone()),
                            vote: VoteChoice::Yes,
                            epoch: 102,
                        },
                    ],
                },
            ],
        },
        
        // Block 3 (Epoch 106): Voting closes, proposal passes
        Block {
            height: 3,
            hash: BlockHash("block_gov_3".to_string()),
            slot: 2_592_000, // Epoch 106
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_gov_3"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 0,
                    metadata: None,
                    governance_actions: vec![
                        // Proposal transitions to Passed
                        GovernanceAction {
                            action_id: ActionId::new("action_k_param_500"),
                            action_type: GovernanceActionType::ParameterChange {
                                parameters: {
                                    let mut params = HashMap::new();
                                    params.insert("k".to_string(), "500".to_string());
                                    params
                                },
                            },
                            proposed_in_epoch: 100,
                            expires_in_epoch: 106,
                            deposit: 100_000_000_000,
                            return_address: alice.clone(),
                            description: "Increase k parameter to 500 for better decentralization".to_string(),
                            status: ProposalStatus::Passed,
                            enacted_epoch: Some(108), // Will enact in epoch 108
                        },
                    ],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 4 (Epoch 108): Proposal enacted
        Block {
            height: 4,
            hash: BlockHash("block_gov_4".to_string()),
            slot: 3_456_000,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_gov_4"),
                    inputs: vec![],
                    outputs: vec![
                        // Return deposit
                        TxOutput {
                            address: alice.clone(),
                            amount: 100_000_000_000,
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 0,
                    metadata: None,
                    governance_actions: vec![
                        GovernanceAction {
                            action_id: ActionId::new("action_k_param_500"),
                            action_type: GovernanceActionType::ParameterChange {
                                parameters: {
                                    let mut params = HashMap::new();
                                    params.insert("k".to_string(), "500".to_string());
                                    params
                                },
                            },
                            proposed_in_epoch: 100,
                            expires_in_epoch: 106,
                            deposit: 100_000_000_000,
                            return_address: alice.clone(),
                            description: "Increase k parameter to 500 for better decentralization".to_string(),
                            status: ProposalStatus::Enacted,
                            enacted_epoch: Some(108),
                        },
                    ],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                },
            ],
        },
    ]
}

/// Generate blocks with staking operations and rewards
pub fn generate_staking_scenario_blocks() -> Vec<Block> {
    let alice = Address::new("addr1_alice");
    let bob = Address::new("addr1_bob");
    
    let alice_stake = StakeKey::new("stake1_alice");
    let bob_stake = StakeKey::new("stake1_bob");
    
    let pool1 = PoolId("pool1xyz".to_string());
    let pool2 = PoolId("pool2abc".to_string());
    
    vec![
        // Block 0 (Epoch 200): Register stake keys and delegate
        Block {
            height: 0,
            hash: BlockHash("block_stake_0".to_string()),
            slot: 0,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_stake_0"),
                    inputs: vec![],
                    outputs: vec![
                        TxOutput {
                            address: alice.clone(),
                            amount: 100_000_000, // 100 ADA
                            assets: vec![],
                            datum: None,
                        },
                        TxOutput {
                            address: bob.clone(),
                            amount: 50_000_000, // 50 ADA
                            assets: vec![],
                            datum: None,
                        },
                    ],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![
                        Certificate::StakeRegistration(StakeRegistration {
                            stake_key: alice_stake.clone(),
                            deposit: 2_000_000, // 2 ADA deposit
                            epoch: 200,
                        }),
                        Certificate::Delegation(DelegationCertificate {
                            stake_key: alice_stake.clone(),
                            pool_id: pool1.clone(),
                            epoch: 200,
                        }),
                        Certificate::StakeRegistration(StakeRegistration {
                            stake_key: bob_stake.clone(),
                            deposit: 2_000_000,
                            epoch: 200,
                        }),
                        Certificate::Delegation(DelegationCertificate {
                            stake_key: bob_stake.clone(),
                            pool_id: pool1.clone(),
                            epoch: 200,
                        }),
                    ],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 1 (Epoch 201): Bob switches delegation to pool2
        Block {
            height: 1,
            hash: BlockHash("block_stake_1".to_string()),
            slot: 432_000,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_stake_1"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![
                        Certificate::Delegation(DelegationCertificate {
                            stake_key: bob_stake.clone(),
                            pool_id: pool2.clone(),
                            epoch: 201,
                        }),
                    ],
                    withdrawals: vec![],
                },
            ],
        },
        
        // Block 2 (Epoch 202): Alice withdraws rewards
        Block {
            height: 2,
            hash: BlockHash("block_stake_2".to_string()),
            slot: 864_000,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_stake_2"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![],
                    withdrawals: vec![
                        (alice_stake.clone(), 3_500_000), // Withdraw 3.5 ADA rewards
                    ],
                },
            ],
        },
        
        // Block 3 (Epoch 203): Bob withdraws rewards  
        Block {
            height: 3,
            hash: BlockHash("block_stake_3".to_string()),
            slot: 1_296_000,
            transactions: vec![
                Transaction {
                    hash: TxHash::new("tx_stake_3"),
                    inputs: vec![],
                    outputs: vec![],
                    fee: 170_000,
                    metadata: None,
                    governance_actions: vec![],
                    votes: vec![],
                    certificates: vec![],
                    withdrawals: vec![],
                    certificates: vec![],
                    withdrawals: vec![
                        (bob_stake.clone(), 2_800_000), // Withdraw 2.8 ADA rewards
                    ],
                },
            ],
        },
    ]
}
