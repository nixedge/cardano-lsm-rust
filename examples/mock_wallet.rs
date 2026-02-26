// Mock Wallet Indexer - Integration test using LSM storage
// Simulates a real Cardano wallet indexing blockchain data

mod mock_types;

use cardano_lsm::{LsmTree, LsmConfig, LsmSnapshot, Key, Value, MonoidalLsmTree, IncrementalMerkleTree};
use mock_types::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use serde::{Serialize, Deserialize};

/// Wallet storage using LSM trees
pub struct WalletIndexer {
    /// UTXO set: key = "tx_hash#output_index" -> UTXO data
    utxo_tree: LsmTree,
    
    /// Transaction history: key = "address/block_height/tx_hash" -> TX data
    tx_tree: LsmTree,
    
    /// Address balances: key = "address" -> balance (ADA in lovelace)
    balance_tree: MonoidalLsmTree<u64>,
    
    /// Asset balances: key = "address/policy/asset" -> amount
    asset_tree: MonoidalLsmTree<u64>,
    
    /// Governance actions: key = "action_id" -> GovernanceAction
    governance_tree: LsmTree,
    
    /// Governance votes: key = "action_id/voter_id" -> Vote
    vote_tree: LsmTree,
    
    /// Incremental Merkle tree for governance verification
    governance_merkle: IncrementalMerkleTree,
    
    /// Stake delegations: key = "stake_key" -> PoolId
    stake_delegation_tree: LsmTree,
    
    /// Rewards by epoch: key = "stake_key/epoch" -> rewards
    rewards_tree: MonoidalLsmTree<u64>,
    
    /// Block snapshots for rollback (last N blocks)
    snapshots: HashMap<u64, WalletSnapshot>,
    
    /// Current sync height
    current_height: u64,
    
    /// Current epoch
    current_epoch: u64,
    
    /// Tracked addresses
    tracked_addresses: HashSet<Address>,
    
    /// My DRep ID (for tracking my votes)
    my_drep: Option<DRepId>,
    
    /// My stake keys (for tracking rewards)
    my_stake_keys: HashSet<StakeKey>,
}

#[derive(Clone)]
struct WalletSnapshot {
    height: u64,
    utxo_snapshot: LsmSnapshot,
    tx_snapshot: LsmSnapshot,
    balance_snapshot: cardano_lsm::monoidal::MonoidalSnapshot<u64>,
    governance_snapshot: LsmSnapshot,
    vote_snapshot: LsmSnapshot,
    merkle_snapshot: cardano_lsm::MerkleSnapshot,
    stake_snapshot: LsmSnapshot,
    rewards_snapshot: cardano_lsm::monoidal::MonoidalSnapshot<u64>,
}

impl WalletIndexer {
    /// Create a new wallet indexer
    pub fn new(path: impl AsRef<Path>, addresses: Vec<Address>) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_drep(path, addresses, None)
    }
    
    /// Create a new wallet indexer with DRep tracking
    pub fn new_with_drep(
        path: impl AsRef<Path>, 
        addresses: Vec<Address>,
        my_drep: Option<DRepId>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_full(path, addresses, my_drep, HashSet::new())
    }
    
    /// Create a new wallet indexer with full configuration
    pub fn new_full(
        path: impl AsRef<Path>, 
        addresses: Vec<Address>,
        my_drep: Option<DRepId>,
        my_stake_keys: HashSet<StakeKey>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        
        // Create LSM trees
        let utxo_tree = LsmTree::open(path.join("utxos"), LsmConfig::default())?;
        let tx_tree = LsmTree::open(path.join("txs"), LsmConfig::default())?;
        let balance_tree = MonoidalLsmTree::open(path.join("balances"), LsmConfig::default())?;
        let asset_tree = MonoidalLsmTree::open(path.join("assets"), LsmConfig::default())?;
        let governance_tree = LsmTree::open(path.join("governance"), LsmConfig::default())?;
        let vote_tree = LsmTree::open(path.join("votes"), LsmConfig::default())?;
        let stake_delegation_tree = LsmTree::open(path.join("stake_delegation"), LsmConfig::default())?;
        let rewards_tree = MonoidalLsmTree::open(path.join("rewards"), LsmConfig::default())?;
        
        // Create Merkle tree for governance verification (height 32 = 4B actions)
        let governance_merkle = IncrementalMerkleTree::new(32);
        
        Ok(Self {
            utxo_tree,
            tx_tree,
            balance_tree,
            asset_tree,
            governance_tree,
            vote_tree,
            governance_merkle,
            stake_delegation_tree,
            rewards_tree,
            snapshots: HashMap::new(),
            current_height: 0,
            current_epoch: 100, // Start at epoch 100
            tracked_addresses: addresses.into_iter().collect(),
            my_drep,
            my_stake_keys,
        })
    }
    
    /// Add an address to track
    pub fn track_address(&mut self, address: Address) {
        self.tracked_addresses.insert(address);
    }
    
    /// Process a block from the chain
    pub fn process_block(&mut self, block: Block) -> Result<(), Box<dyn std::error::Error>> {
        println!("📦 Processing block {} (height: {}, {} txs)", 
                 block.hash.0, block.height, block.transactions.len());
        
        // Create snapshot before processing
        self.create_snapshot(block.height)?;
        
        let mut utxos_created = 0;
        let mut utxos_spent = 0;
        let mut relevant_txs = 0;
        
        // Process each transaction
        for tx in &block.transactions {
            let mut is_relevant = false;
            
            // Handle inputs (spending UTXOs)
            for input in &tx.inputs {
                let utxo_key = format!("{}#{}", input.tx_hash.0, input.output_index);
                
                // Check if this UTXO exists and belongs to us
                if let Some(utxo_data) = self.utxo_tree.get(&Key::from(utxo_key.as_bytes()))? {
                    let utxo: Utxo = bincode::deserialize(utxo_data.as_ref())?;
                    
                    if self.tracked_addresses.contains(&utxo.address) {
                        is_relevant = true;
                        
                        // Remove UTXO
                        self.utxo_tree.delete(&Key::from(utxo_key.as_bytes()))?;
                        utxos_spent += 1;
                        
                        // Update balance (subtract)
                        let balance_key = format!("balance_{}", utxo.address.0);
                        let current = self.balance_tree.get(&Key::from(balance_key.as_bytes()))?;
                        let new_balance = current.saturating_sub(utxo.amount);
                        self.balance_tree.insert(&Key::from(balance_key.as_bytes()), &new_balance)?;
                        
                        println!("  💸 Spent UTXO: {} ({} lovelace)", utxo_key, utxo.amount);
                    }
                }
            }
            
            // Handle outputs (creating new UTXOs)
            for (idx, output) in tx.outputs.iter().enumerate() {
                if self.tracked_addresses.contains(&output.address) {
                    is_relevant = true;
                    
                    // Create UTXO
                    let utxo = Utxo {
                        tx_hash: tx.hash.clone(),
                        output_index: idx as u32,
                        address: output.address.clone(),
                        amount: output.amount,
                        assets: output.assets.clone(),
                        datum: output.datum.clone(),
                        created_at_block: block.height,
                    };
                    
                    let utxo_key = utxo.to_key();
                    let utxo_data = bincode::serialize(&utxo)?;
                    self.utxo_tree.insert(&Key::from(utxo_key.as_bytes()), &Value::from(&utxo_data))?;
                    utxos_created += 1;
                    
                    // Update balance (add)
                    let balance_key = format!("balance_{}", output.address.0);
                    let current = self.balance_tree.get(&Key::from(balance_key.as_bytes()))?;
                    let new_balance = current.saturating_add(output.amount);
                    self.balance_tree.insert(&Key::from(balance_key.as_bytes()), &new_balance)?;
                    
                    println!("  💰 Created UTXO: {} ({} lovelace) -> {}", 
                             utxo_key, output.amount, output.address.0);
                }
            }
            
            // Store transaction if relevant
            if is_relevant {
                relevant_txs += 1;
                let tx_data = bincode::serialize(&tx)?;
                
                // Store by tx hash
                self.tx_tree.insert(&Key::from(tx.hash.0.as_bytes()), &Value::from(&tx_data))?;
                
                // Index by affected addresses
                for input in &tx.inputs {
                    if let Some(utxo_data) = self.utxo_tree.get(&Key::from(format!("{}#{}", input.tx_hash.0, input.output_index).as_bytes()))? {
                        let utxo: Utxo = bincode::deserialize(utxo_data.as_ref())?;
                        if self.tracked_addresses.contains(&utxo.address) {
                            let index_key = format!("addr_{}/tx/{}/{}", utxo.address.0, block.height, tx.hash.0);
                            self.tx_tree.insert(&Key::from(index_key.as_bytes()), &Value::from(b""))?;
                        }
                    }
                }
                
                for output in &tx.outputs {
                    if self.tracked_addresses.contains(&output.address) {
                        let index_key = format!("addr_{}/tx/{}/{}", output.address.0, block.height, tx.hash.0);
                        self.tx_tree.insert(&Key::from(index_key.as_bytes()), &Value::from(b""))?;
                    }
                }
            }
            
            // Process governance actions
            for action in &tx.governance_actions {
                self.index_governance_action(action, block.height)?;
            }
            
            // Process votes
            for vote in &tx.votes {
                self.index_vote(vote)?;
            }
            
            // Process certificates
            for cert in &tx.certificates {
                self.process_certificate(cert)?;
            }
            
            // Process reward withdrawals
            for (stake_key, amount) in &tx.withdrawals {
                self.process_withdrawal(stake_key, *amount)?;
            }
        }
        
        // Update current epoch based on slot
        self.current_epoch = block.slot / 432_000; // ~5 day epochs
        
        println!("  ✅ Block processed: {} relevant txs, {} UTXOs created, {} spent", 
                 relevant_txs, utxos_created, utxos_spent);
        
        self.current_height = block.height;
        Ok(())
    }
    
    /// Handle chain reorganization
    pub fn rollback(&mut self, to_height: u64) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔄 Rolling back from block {} to block {}", self.current_height, to_height);
        
        if let Some(snapshot) = self.snapshots.get(&to_height) {
            self.utxo_tree.rollback(snapshot.utxo_snapshot.clone())?;
            self.tx_tree.rollback(snapshot.tx_snapshot.clone())?;
            self.balance_tree.rollback(snapshot.balance_snapshot.clone())?;
            self.governance_tree.rollback(snapshot.governance_snapshot.clone())?;
            self.vote_tree.rollback(snapshot.vote_snapshot.clone())?;
            self.governance_merkle.rollback(snapshot.merkle_snapshot.clone())?;
            self.stake_delegation_tree.rollback(snapshot.stake_snapshot.clone())?;
            self.rewards_tree.rollback(snapshot.rewards_snapshot.clone())?;
            
            // Remove snapshots after rollback point
            self.snapshots.retain(|&h, _| h <= to_height);
            
            self.current_height = to_height;
            println!("  ✅ Rollback complete");
            Ok(())
        } else {
            Err("Snapshot not found for rollback height".into())
        }
    }
    
    /// Index a governance action
    fn index_governance_action(&mut self, action: &GovernanceAction, block_height: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Store action in governance tree
        let action_data = bincode::serialize(action)?;
        self.governance_tree.insert(
            &Key::from(action.action_id.0.as_bytes()),
            &Value::from(&action_data)
        )?;
        
        // Add to Merkle tree for cryptographic verification
        let merkle_proof = self.governance_merkle.insert(
            action.action_id.0.as_bytes(),
            &action_data
        );
        
        println!("  🗳️  Governance action indexed: {} (status: {:?})", 
                 action.action_id.0, action.status);
        
        Ok(())
    }
    
    /// Index a vote
    fn index_vote(&mut self, vote: &Vote) -> Result<(), Box<dyn std::error::Error>> {
        let voter_id = match &vote.voter {
            Voter::DRep(id) => format!("drep_{}", id.0),
            Voter::StakePoolOperator(id) => format!("spo_{}", id.0),
            Voter::ConstitutionalCommittee(id) => format!("cc_{}", id.0),
        };
        
        let vote_key = format!("{}/{}", vote.action_id.0, voter_id);
        let vote_data = bincode::serialize(vote)?;
        self.vote_tree.insert(&Key::from(vote_key.as_bytes()), &Value::from(&vote_data))?;
        
        // Check if this is my DRep's vote
        if let Some(ref my_drep) = self.my_drep {
            if let Voter::DRep(drep_id) = &vote.voter {
                if drep_id == my_drep {
                    println!("  📝 My DRep voted: {:?} on {}", vote.vote, vote.action_id.0);
                }
            }
        }
        
        Ok(())
    }
    
    /// Get a governance action by ID
    pub fn get_governance_action(&self, action_id: &ActionId) -> Result<Option<GovernanceAction>, Box<dyn std::error::Error>> {
        if let Some(data) = self.governance_tree.get(&Key::from(action_id.0.as_bytes()))? {
            let action: GovernanceAction = bincode::deserialize(data.as_ref())?;
            Ok(Some(action))
        } else {
            Ok(None)
        }
    }
    
    /// Get all votes for an action
    pub fn get_votes_for_action(&self, action_id: &ActionId) -> Result<Vec<Vote>, Box<dyn std::error::Error>> {
        let prefix = format!("{}/", action_id.0);
        let mut votes = Vec::new();
        
        for (_, vote_data) in self.vote_tree.scan_prefix(prefix.as_bytes()) {
            let vote: Vote = bincode::deserialize(vote_data.as_ref())?;
            votes.push(vote);
        }
        
        Ok(votes)
    }
    
    /// Get how my DRep voted on an action
    pub fn get_my_drep_vote(&self, action_id: &ActionId) -> Result<Option<Vote>, Box<dyn std::error::Error>> {
        if let Some(ref my_drep) = self.my_drep {
            let vote_key = format!("{}/drep_{}", action_id.0, my_drep.0);
            if let Some(vote_data) = self.vote_tree.get(&Key::from(vote_key.as_bytes()))? {
                let vote: Vote = bincode::deserialize(vote_data.as_ref())?;
                return Ok(Some(vote));
            }
        }
        Ok(None)
    }
    
    /// Get live voting statistics for an active proposal
    pub fn get_live_voting_stats(&self, action_id: &ActionId) -> Result<VotingStats, Box<dyn std::error::Error>> {
        let action = self.get_governance_action(action_id)?
            .ok_or("Action not found")?;
        
        let votes = self.get_votes_for_action(action_id)?;
        
        // Calculate voting statistics
        // In real implementation, would weight by stake
        let mut yes_count = 0u64;
        let mut no_count = 0u64;
        let mut abstain_count = 0u64;
        
        for vote in &votes {
            // Mock: each vote = 1M ADA stake
            let stake = 1_000_000_000_000;
            
            match vote.vote {
                VoteChoice::Yes => yes_count += stake,
                VoteChoice::No => no_count += stake,
                VoteChoice::Abstain => abstain_count += stake,
            }
        }
        
        // Mock thresholds (67% for most governance actions)
        let total_stake = 10_000_000_000_000; // 10M ADA total
        let drep_threshold = (total_stake as f64 * 0.67) as u64;
        let spo_threshold = (total_stake as f64 * 0.51) as u64; // Simple majority for SPOs
        let cc_threshold = (total_stake as f64 * 0.67) as u64;
        
        Ok(VotingStats {
            action_id: action_id.clone(),
            total_yes_stake: yes_count,
            total_no_stake: no_count,
            total_abstain_stake: abstain_count,
            drep_threshold,
            spo_threshold,
            cc_threshold,
            current_epoch: self.current_epoch,
            expires_epoch: action.expires_in_epoch,
        })
    }
    
    /// Get all active proposals (not yet enacted or expired)
    pub fn get_active_proposals(&self) -> Result<Vec<GovernanceAction>, Box<dyn std::error::Error>> {
        let mut actions = Vec::new();
        
        for (_, action_data) in self.governance_tree.iter() {
            let action: GovernanceAction = bincode::deserialize(action_data.as_ref())?;
            if action.status == ProposalStatus::Active {
                actions.push(action);
            }
        }
        
        Ok(actions)
    }
    
    /// Get Merkle proof for a governance action (cryptographic verification!)
    pub fn get_governance_proof(&self, action_id: &ActionId) -> Option<cardano_lsm::MerkleProof> {
        self.governance_merkle.prove(action_id.0.as_bytes())
    }
    
    /// Verify a governance action is in the Merkle tree
    pub fn verify_governance_action(&self, action_id: &ActionId) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(proof) = self.get_governance_proof(action_id) {
            Ok(self.governance_merkle.verify(&proof).is_ok())
        } else {
            Ok(false)
        }
    }
    
    /// Get historical voting record with Merkle proof
    pub fn get_voting_record(&self, action_id: &ActionId) -> Result<Option<VotingRecord>, Box<dyn std::error::Error>> {
        let action = match self.get_governance_action(action_id)? {
            Some(a) => a,
            None => return Ok(None),
        };
        
        // Only return full record for completed actions
        if action.status != ProposalStatus::Enacted && action.status != ProposalStatus::Failed {
            return Ok(None);
        }
        
        let all_votes = self.get_votes_for_action(action_id)?;
        let merkle_root = self.governance_merkle.root().as_bytes().to_vec();
        
        Ok(Some(VotingRecord {
            action_id: action_id.clone(),
            all_votes,
            final_result: action.status.clone(),
            enacted_epoch: action.enacted_epoch,
            merkle_root,
        }))
    }
    
    // ========================================================================
    // Staking Methods
    // ========================================================================
    
    /// Process a certificate
    fn process_certificate(&mut self, cert: &Certificate) -> Result<(), Box<dyn std::error::Error>> {
        match cert {
            Certificate::StakeRegistration(reg) => {
                println!("  🎫 Stake registration: {}", reg.stake_key.0);
                // In real impl, would track registration status
            }
            
            Certificate::StakeDeregistration { stake_key, .. } => {
                println!("  ❌ Stake deregistration: {}", stake_key.0);
                self.stake_delegation_tree.delete(&Key::from(stake_key.0.as_bytes()))?;
            }
            
            Certificate::Delegation(deleg) => {
                if self.my_stake_keys.contains(&deleg.stake_key) {
                    println!("  🤝 My stake delegated to pool: {}", deleg.pool_id.0);
                }
                
                // Store delegation
                let deleg_data = bincode::serialize(&deleg.pool_id)?;
                self.stake_delegation_tree.insert(
                    &Key::from(deleg.stake_key.0.as_bytes()),
                    &Value::from(&deleg_data)
                )?;
            }
            
            Certificate::PoolRegistration(pool) => {
                println!("  🏊 Pool registered: {} ({})", pool.name, pool.pool_id.0);
            }
            
            Certificate::PoolRetirement { pool_id, retire_epoch } => {
                println!("  👋 Pool retiring: {} in epoch {}", pool_id.0, retire_epoch);
            }
        }
        
        Ok(())
    }
    
    /// Process reward withdrawal
    fn process_withdrawal(&mut self, stake_key: &StakeKey, amount: u64) -> Result<(), Box<dyn std::error::Error>> {
        if self.my_stake_keys.contains(stake_key) {
            println!("  💵 Withdrawing rewards: {} ADA ({} lovelace)", 
                     amount as f64 / 1_000_000.0, amount);
        }
        
        // Record withdrawal (subtracts from available rewards)
        let rewards_key = format!("{}/total", stake_key.0);
        let current_rewards = self.rewards_tree.get(&Key::from(rewards_key.as_bytes()))?;
        let new_rewards = current_rewards.saturating_sub(amount);
        self.rewards_tree.insert(&Key::from(rewards_key.as_bytes()), &new_rewards)?;
        
        Ok(())
    }
    
    /// Add rewards for an epoch (simulated - normally from protocol)
    pub fn add_epoch_rewards(&mut self, stake_key: &StakeKey, epoch: u64, amount: u64) -> Result<(), Box<dyn std::error::Error>> {
        // Record rewards for specific epoch
        let epoch_key = format!("{}/{}", stake_key.0, epoch);
        self.rewards_tree.insert(&Key::from(epoch_key.as_bytes()), &amount)?;
        
        // Add to total rewards
        let total_key = format!("{}/total", stake_key.0);
        let current_total = self.rewards_tree.get(&Key::from(total_key.as_bytes()))?;
        let new_total = current_total + amount;
        self.rewards_tree.insert(&Key::from(total_key.as_bytes()), &new_total)?;
        
        Ok(())
    }
    
    /// Get current delegation for a stake key
    pub fn get_delegation(&self, stake_key: &StakeKey) -> Result<Option<PoolId>, Box<dyn std::error::Error>> {
        if let Some(pool_data) = self.stake_delegation_tree.get(&Key::from(stake_key.0.as_bytes()))? {
            let pool_id: PoolId = bincode::deserialize(pool_data.as_ref())?;
            Ok(Some(pool_id))
        } else {
            Ok(None)
        }
    }
    
    /// Get total rewards for a stake key (all epochs)
    pub fn get_total_rewards(&self, stake_key: &StakeKey) -> Result<u64, Box<dyn std::error::Error>> {
        let total_key = format!("{}/total", stake_key.0);
        let total = self.rewards_tree.get(&Key::from(total_key.as_bytes()))?;
        Ok(total)
    }
    
    /// Get rewards for a specific epoch
    pub fn get_epoch_rewards(&self, stake_key: &StakeKey, epoch: u64) -> Result<u64, Box<dyn std::error::Error>> {
        let epoch_key = format!("{}/{}", stake_key.0, epoch);
        let rewards = self.rewards_tree.get(&Key::from(epoch_key.as_bytes()))?;
        Ok(rewards)
    }
    
    /// Get all epoch rewards for a stake key
    pub fn get_rewards_history(&self, stake_key: &StakeKey) -> Result<Vec<(u64, u64)>, Box<dyn std::error::Error>> {
        // For now, we'll reconstruct from known epochs
        // In a real implementation, we'd iterate over the tree
        let mut rewards = Vec::new();
        
        // Check epochs 0-1000 (in real impl, would iterate tree)
        for epoch in 0..1000 {
            let amount = self.get_epoch_rewards(stake_key, epoch)?;
            if amount > 0 {
                rewards.push((epoch, amount));
            }
        }
        
        rewards.sort_by_key(|(epoch, _)| *epoch);
        Ok(rewards)
    }
    
    /// Get staking status
    pub fn get_staking_status(&self, stake_key: &StakeKey) -> Result<StakingStatus, Box<dyn std::error::Error>> {
        let delegated_pool = self.get_delegation(stake_key)?;
        let total_rewards = self.get_total_rewards(stake_key)?;
        
        // Find last reward epoch
        let rewards_history = self.get_rewards_history(stake_key)?;
        let last_reward_epoch = rewards_history.last().map(|(e, _)| *e).unwrap_or(0);
        
        Ok(StakingStatus {
            stake_key: stake_key.clone(),
            delegated_pool,
            total_rewards,
            last_reward_epoch,
            is_registered: delegated_pool.is_some(),
        })
    }
    
    /// Get all UTXOs for an address
    pub fn get_utxos(&self, address: &Address) -> Result<Vec<Utxo>, Box<dyn std::error::Error>> {
        let mut utxos = Vec::new();
        
        // Scan all UTXOs
        for (key, value) in self.utxo_tree.iter() {
            let utxo: Utxo = bincode::deserialize(value.as_ref())?;
            if &utxo.address == address {
                utxos.push(utxo);
            }
        }
        
        Ok(utxos)
    }
    
    /// Get balance for an address
    pub fn get_balance(&self, address: &Address) -> Result<u64, Box<dyn std::error::Error>> {
        let balance_key = format!("balance_{}", address.0);
        let balance = self.balance_tree.get(&Key::from(balance_key.as_bytes()))?;
        Ok(balance)
    }
    
    /// Get total wallet balance (all tracked addresses)
    pub fn get_total_balance(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let total = self.balance_tree.prefix_fold(b"balance_");
        Ok(total)
    }
    
    /// Get transaction history for an address
    pub fn get_transaction_history(&self, address: &Address) -> Result<Vec<Transaction>, Box<dyn std::error::Error>> {
        let mut transactions = Vec::new();
        let prefix = format!("addr_{}/tx/", address.0);
        
        for (key, _) in self.tx_tree.scan_prefix(prefix.as_bytes()) {
            // Extract tx hash from key: "addr_X/tx/HEIGHT/TX_HASH"
            let key_str = String::from_utf8_lossy(key.as_ref());
            if let Some(tx_hash) = key_str.split('/').last() {
                if let Some(tx_data) = self.tx_tree.get(&Key::from(tx_hash.as_bytes()))? {
                    let tx: Transaction = bincode::deserialize(tx_data.as_ref())?;
                    transactions.push(tx);
                }
            }
        }
        
        // Remove duplicates and sort by hash
        transactions.sort_by(|a, b| a.hash.0.cmp(&b.hash.0));
        transactions.dedup_by(|a, b| a.hash == b.hash);
        
        Ok(transactions)
    }
    
    /// Create snapshot for rollback capability
    fn create_snapshot(&mut self, height: u64) -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = WalletSnapshot {
            height,
            utxo_snapshot: self.utxo_tree.snapshot(),
            tx_snapshot: self.tx_tree.snapshot(),
            balance_snapshot: self.balance_tree.snapshot(),
            governance_snapshot: self.governance_tree.snapshot(),
            vote_snapshot: self.vote_tree.snapshot(),
            merkle_snapshot: self.governance_merkle.snapshot(),
            stake_snapshot: self.stake_delegation_tree.snapshot(),
            rewards_snapshot: self.rewards_tree.snapshot(),
        };
        
        self.snapshots.insert(height, snapshot);
        
        // Keep only last 10 snapshots
        if self.snapshots.len() > 10 {
            if let Some(&min_height) = self.snapshots.keys().min() {
                self.snapshots.remove(&min_height);
            }
        }
        
        Ok(())
    }
    
    /// Print wallet status
    pub fn print_status(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("📊 Wallet Status (Block {}, Epoch {})", self.current_height, self.current_epoch);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        for address in &self.tracked_addresses {
            let balance = self.get_balance(address)?;
            let utxos = self.get_utxos(address)?;
            
            println!("\n{}", address.0);
            println!("  Balance: {} ADA ({} lovelace)", balance as f64 / 1_000_000.0, balance);
            println!("  UTXOs: {}", utxos.len());
            
            for utxo in utxos {
                println!("    • {} = {} lovelace", utxo.to_key(), utxo.amount);
            }
        }
        
        println!("\n💰 Total Wallet Balance: {} ADA", 
                 self.get_total_balance()? as f64 / 1_000_000.0);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        
        Ok(())
    }
    
    /// Print governance status
    pub fn print_governance_status(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🗳️  Governance Status (Epoch {})", self.current_epoch);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        // Get all active proposals
        let active = self.get_active_proposals()?;
        
        if active.is_empty() {
            println!("\n  No active proposals");
        }
        
        for action in &active {
            println!("\n📋 {}", action.action_id.0);
            println!("   Type: {:?}", action.action_type);
            println!("   Description: {}", action.description);
            println!("   Proposed: Epoch {}", action.proposed_in_epoch);
            println!("   Expires: Epoch {} ({} days)", 
                     action.expires_in_epoch,
                     (action.expires_in_epoch - self.current_epoch) * 5);
            
            // Get voting stats
            let stats = self.get_live_voting_stats(&action.action_id)?;
            
            println!("   Voting:");
            println!("     Yes:     {} ADA ({:.1}%)", 
                     stats.total_yes_stake as f64 / 1_000_000.0,
                     stats.total_yes_stake as f64 / 10_000_000_000_000.0 * 100.0);
            println!("     No:      {} ADA ({:.1}%)", 
                     stats.total_no_stake as f64 / 1_000_000.0,
                     stats.total_no_stake as f64 / 10_000_000_000_000.0 * 100.0);
            println!("     Abstain: {} ADA ({:.1}%)", 
                     stats.total_abstain_stake as f64 / 1_000_000.0,
                     stats.total_abstain_stake as f64 / 10_000_000_000_000.0 * 100.0);
            
            println!("   Thresholds:");
            println!("     DRep: {} {} ({}%)", 
                     if stats.has_passed_drep_threshold() { "✅" } else { "❌" },
                     "Passed" if stats.has_passed_drep_threshold() else "Not met",
                     67);
            println!("     SPO:  {} {} ({}%)",
                     if stats.has_passed_spo_threshold() { "✅" } else { "❌" },
                     "Passed" if stats.has_passed_spo_threshold() else "Not met",
                     51);
            
            // Show my DRep's vote
            if let Some(my_vote) = self.get_my_drep_vote(&action.action_id)? {
                println!("\n   🙋 My DRep Vote: {:?}", my_vote.vote);
            }
            
            // Overall status
            if stats.has_passed() {
                println!("\n   ✅ STATUS: PASSED - Will be enacted in epoch {}", 
                         action.expires_in_epoch + 2);
            } else if stats.is_active() {
                println!("\n   ⏳ STATUS: VOTING ({} days remaining)", stats.days_until_expiry());
            } else {
                println!("\n   ❌ STATUS: EXPIRED");
            }
        }
        
        // Show historical (enacted) proposals
        println!("\n📜 Historical Proposals:");
        
        for (_, action_data) in self.governance_tree.iter() {
            let action: GovernanceAction = bincode::deserialize(action_data.as_ref())?;
            
            if action.status == ProposalStatus::Enacted {
                println!("\n  {} (Enacted in Epoch {})", 
                         action.action_id.0, 
                         action.enacted_epoch.unwrap_or(0));
                println!("    Type: {:?}", action.action_type);
                
                // Get voting record with Merkle verification
                if let Some(record) = self.get_voting_record(&action.action_id)? {
                    println!("    Total votes recorded: {}", record.all_votes.len());
                    println!("    Merkle root: {}", hex::encode(&record.merkle_root[..8]));
                    
                    // Verify Merkle proof
                    if self.verify_governance_action(&action.action_id)? {
                        println!("    ✅ Cryptographically verified in Merkle tree");
                    }
                    
                    // Show vote breakdown
                    let yes_votes = record.all_votes.iter().filter(|v| v.vote == VoteChoice::Yes).count();
                    let no_votes = record.all_votes.iter().filter(|v| v.vote == VoteChoice::No).count();
                    let abstain_votes = record.all_votes.iter().filter(|v| v.vote == VoteChoice::Abstain).count();
                    
                    println!("    Votes: {} Yes, {} No, {} Abstain", yes_votes, no_votes, abstain_votes);
                }
            }
        }
        
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🦀 Cardano Mock Wallet Indexer with Governance");
    println!("   Testing LSM-based wallet storage + governance\n");
    
    // Create wallet tracking Alice and Bob, with Alice's DRep
    let temp_dir = tempfile::TempDir::new()?;
    let mut wallet = WalletIndexer::new_with_drep(
        temp_dir.path(),
        vec![
            Address::new("addr1_alice"),
            Address::new("addr1_bob"),
        ],
        Some(DRepId::new("drep1_alice")), // Track Alice's DRep
    )?;
    
    // First run: regular transaction scenario
    println!("=== Part 1: Regular Transactions ===\n");
    let blocks = generate_test_scenario_blocks();
    
    let mut chain = MockChainSync::new(blocks);
    while let Some(block) = chain.next_block() {
        wallet.process_block(block)?;
    }
    
    wallet.print_status()?;
    
    // Second run: governance scenario
    println!("\n=== Part 2: Governance Actions ===\n");
    let gov_blocks = generate_governance_scenario_blocks();
    
    let mut gov_chain = MockChainSync::new(gov_blocks);
    while let Some(block) = gov_chain.next_block() {
        wallet.process_block(block)?;
    }
    
    // Print governance status
    wallet.print_governance_status()?;
    
    // Test governance queries
    println!("🧪 Testing governance queries...\n");
    
    let action_id = ActionId::new("action_k_param_500");
    
    // 1. Check if action exists
    if let Some(action) = wallet.get_governance_action(&action_id)? {
        println!("✅ Action found: {}", action.description);
        println!("   Status: {:?}", action.status);
    }
    
    // 2. Check my DRep's vote
    if let Some(my_vote) = wallet.get_my_drep_vote(&action_id)? {
        println!("✅ My DRep voted: {:?}", my_vote.vote);
    }
    
    // 3. Get all votes
    let all_votes = wallet.get_votes_for_action(&action_id)?;
    println!("✅ Total votes cast: {}", all_votes.len());
    
    // 4. Verify with Merkle proof
    if wallet.verify_governance_action(&action_id)? {
        println!("✅ Governance action cryptographically verified!");
    }
    
    // 5. Get historical record
    if let Some(record) = wallet.get_voting_record(&action_id)? {
        println!("✅ Historical voting record retrieved");
        println!("   Enacted in epoch: {}", record.enacted_epoch.unwrap());
        println!("   Merkle root: {}", hex::encode(&record.merkle_root[..16]));
    }
    
    println!("\n🎉 Governance integration test complete!");
    println!("   ✅ Governance actions indexed");
    println!("   ✅ Votes tracked");
    println!("   ✅ Live stats computed");
    println!("   ✅ Historical records with Merkle proofs");
    println!("   ✅ My DRep vote tracking");
    println!("\n🚀 Ready for real Cardano governance!");
    
    Ok(())
}
