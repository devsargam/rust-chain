use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Hash = [u8; 32];

pub const DEFAULT_INITIAL_REWARD: u64 = 50;
pub const DEFAULT_HALVING_INTERVAL: u64 = 5;

const EMPTY_HASH: Hash = [0; 32];
const GENESIS_MEMO: &str = "genesis";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transaction {
    Reward {
        to: String,
        amount: u64,
    },
    Transfer {
        from: String,
        to: String,
        amount: u64,
    },
    Memo(String),
}

impl Transaction {
    pub fn reward(to: impl Into<String>, amount: u64) -> Self {
        Self::Reward {
            to: to.into(),
            amount,
        }
    }

    pub fn transfer(from: impl Into<String>, to: impl Into<String>, amount: u64) -> Self {
        Self::Transfer {
            from: from.into(),
            to: to.into(),
            amount,
        }
    }

    pub fn memo(text: impl Into<String>) -> Self {
        Self::Memo(text.into())
    }

    fn canonical_string(&self) -> String {
        match self {
            Self::Reward { to, amount } => format!("reward|{to}|{amount}"),
            Self::Transfer { from, to, amount } => format!("transfer|{from}|{to}|{amount}"),
            Self::Memo(text) => format!("memo|{text}"),
        }
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reward { to, amount } => write!(f, "reward -> {to} ({amount})"),
            Self::Transfer { from, to, amount } => write!(f, "{from} -> {to} ({amount})"),
            Self::Memo(text) => write!(f, "memo: {text}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub index: u64,
    pub prev_hash: Hash,
    pub merkle_root: Hash,
    pub timestamp_secs: u64,
    pub nonce: u64,
    pub hash: Hash,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blockchain {
    blocks: Vec<Block>,
    difficulty_bits: u32,
    initial_reward: u64,
    halving_interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStats {
    pub height: u64,
    pub total_blocks: usize,
    pub total_transactions: usize,
    pub reward_transactions: usize,
    pub transfer_transactions: usize,
    pub memo_transactions: usize,
    pub difficulty_bits: u32,
    pub circulating_supply: u64,
    pub unique_accounts: usize,
    pub richest_account: Option<String>,
    pub richest_balance: u64,
    pub next_block_reward: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountActivityKind {
    Reward,
    Sent,
    Received,
}

impl fmt::Display for AccountActivityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reward => f.write_str("reward"),
            Self::Sent => f.write_str("sent"),
            Self::Received => f.write_str("received"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountActivity {
    pub block_index: u64,
    pub kind: AccountActivityKind,
    pub counterparty: Option<String>,
    pub amount: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStatement {
    pub account: String,
    pub balance: u64,
    pub mined_rewards: u64,
    pub transfers_sent: u64,
    pub transfers_received: u64,
    pub activity: Vec<AccountActivity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    InvalidDifficulty(u32),
    InvalidRewardSchedule {
        initial_reward: u64,
        halving_interval: u64,
    },
    NoWorkers,
    InvalidBlock {
        index: u64,
        reason: &'static str,
    },
    InsufficientFunds {
        block_index: u64,
        account: String,
        available: u64,
        required: u64,
    },
    MiningFailed,
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDifficulty(bits) => {
                write!(f, "difficulty must be between 0 and 256 bits, got {bits}")
            }
            Self::InvalidRewardSchedule {
                initial_reward,
                halving_interval,
            } => write!(
                f,
                "reward schedule must use a non-zero halving interval, got initial_reward={initial_reward}, halving_interval={halving_interval}"
            ),
            Self::NoWorkers => f.write_str("mining requires at least one worker"),
            Self::InvalidBlock { index, reason } => {
                write!(f, "block {index} is invalid: {reason}")
            }
            Self::InsufficientFunds {
                block_index,
                account,
                available,
                required,
            } => write!(
                f,
                "block {block_index} overspends account {account}: available={available}, required={required}"
            ),
            Self::MiningFailed => f.write_str("mining finished without producing a block"),
        }
    }
}

impl std::error::Error for ChainError {}

impl Blockchain {
    pub fn new(difficulty_bits: u32) -> Result<Self, ChainError> {
        Self::with_consensus(
            difficulty_bits,
            DEFAULT_INITIAL_REWARD,
            DEFAULT_HALVING_INTERVAL,
        )
    }

    pub fn with_consensus(
        difficulty_bits: u32,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Result<Self, ChainError> {
        validate_difficulty(difficulty_bits)?;
        validate_reward_schedule(initial_reward, halving_interval)?;

        let genesis_transactions = vec![Transaction::memo(GENESIS_MEMO)];
        let genesis_merkle_root = calculate_merkle_root(&genesis_transactions);
        let genesis = Block {
            index: 0,
            prev_hash: EMPTY_HASH,
            merkle_root: genesis_merkle_root,
            timestamp_secs: 0,
            nonce: 0,
            hash: calculate_hash(0, &EMPTY_HASH, &genesis_merkle_root, 0, 0),
            transactions: genesis_transactions,
        };

        Ok(Self {
            blocks: vec![genesis],
            difficulty_bits,
            initial_reward,
            halving_interval,
        })
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn difficulty_bits(&self) -> u32 {
        self.difficulty_bits
    }

    pub fn initial_reward(&self) -> u64 {
        self.initial_reward
    }

    pub fn halving_interval(&self) -> u64 {
        self.halving_interval
    }

    pub fn tip(&self) -> &Block {
        self.blocks
            .last()
            .expect("blockchain is always initialized with a genesis block")
    }

    pub fn block(&self, index: u64) -> Option<&Block> {
        self.blocks
            .get(index as usize)
            .filter(|block| block.index == index)
    }

    pub fn block_reward(&self, block_index: u64) -> u64 {
        if block_index == 0 || self.initial_reward == 0 {
            return 0;
        }

        let halvings = (block_index - 1) / self.halving_interval;
        if halvings >= u64::BITS as u64 {
            0
        } else {
            self.initial_reward
                .checked_shr(halvings as u32)
                .unwrap_or(0)
        }
    }

    pub fn balances(&self) -> Result<HashMap<String, u64>, ChainError> {
        self.validate_and_build_ledger()
    }

    pub fn balance_of(&self, account: &str) -> Result<u64, ChainError> {
        Ok(self.balances()?.get(account).copied().unwrap_or(0))
    }

    pub fn top_accounts(&self, limit: usize) -> Result<Vec<(String, u64)>, ChainError> {
        let mut balances: Vec<_> = self.balances()?.into_iter().collect();
        balances.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        balances.truncate(limit);
        Ok(balances)
    }

    pub fn stats(&self) -> Result<ChainStats, ChainError> {
        let balances = self.validate_and_build_ledger()?;
        let mut reward_transactions = 0usize;
        let mut transfer_transactions = 0usize;
        let mut memo_transactions = 0usize;

        for transaction in self
            .blocks
            .iter()
            .flat_map(|block| block.transactions.iter())
        {
            match transaction {
                Transaction::Reward { .. } => reward_transactions += 1,
                Transaction::Transfer { .. } => transfer_transactions += 1,
                Transaction::Memo(_) => memo_transactions += 1,
            }
        }

        let circulating_supply = balances.values().try_fold(0u64, |supply, balance| {
            supply
                .checked_add(*balance)
                .ok_or(ChainError::InvalidBlock {
                    index: self.tip().index,
                    reason: "circulating supply overflowed",
                })
        })?;

        let (richest_account, richest_balance) = balances
            .iter()
            .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
            .map(|(account, balance)| (Some(account.clone()), *balance))
            .unwrap_or((None, 0));

        Ok(ChainStats {
            height: self.tip().index,
            total_blocks: self.blocks.len(),
            total_transactions: reward_transactions + transfer_transactions + memo_transactions,
            reward_transactions,
            transfer_transactions,
            memo_transactions,
            difficulty_bits: self.difficulty_bits,
            circulating_supply,
            unique_accounts: balances.len(),
            richest_account,
            richest_balance,
            next_block_reward: self.block_reward(self.tip().index + 1),
        })
    }

    pub fn account_statement(&self, account: &str) -> Result<AccountStatement, ChainError> {
        let balances = self.validate_and_build_ledger()?;
        let mut mined_rewards = 0u64;
        let mut transfers_sent = 0u64;
        let mut transfers_received = 0u64;
        let mut activity = Vec::new();

        for block in self.blocks.iter().skip(1) {
            for transaction in &block.transactions {
                match transaction {
                    Transaction::Reward { to, amount } if to == account => {
                        mined_rewards =
                            mined_rewards
                                .checked_add(*amount)
                                .ok_or(ChainError::InvalidBlock {
                                    index: block.index,
                                    reason: "account balance overflowed",
                                })?;
                        activity.push(AccountActivity {
                            block_index: block.index,
                            kind: AccountActivityKind::Reward,
                            counterparty: None,
                            amount: *amount,
                        });
                    }
                    Transaction::Transfer { from, to, amount } if from == account => {
                        transfers_sent = transfers_sent.checked_add(*amount).ok_or(
                            ChainError::InvalidBlock {
                                index: block.index,
                                reason: "account balance overflowed",
                            },
                        )?;
                        activity.push(AccountActivity {
                            block_index: block.index,
                            kind: AccountActivityKind::Sent,
                            counterparty: Some(to.clone()),
                            amount: *amount,
                        });
                    }
                    Transaction::Transfer { from, to, amount } if to == account => {
                        transfers_received = transfers_received.checked_add(*amount).ok_or(
                            ChainError::InvalidBlock {
                                index: block.index,
                                reason: "account balance overflowed",
                            },
                        )?;
                        activity.push(AccountActivity {
                            block_index: block.index,
                            kind: AccountActivityKind::Received,
                            counterparty: Some(from.clone()),
                            amount: *amount,
                        });
                    }
                    _ => {}
                }
            }
        }

        Ok(AccountStatement {
            account: account.to_owned(),
            balance: balances.get(account).copied().unwrap_or(0),
            mined_rewards,
            transfers_sent,
            transfers_received,
            activity,
        })
    }

    pub fn mine_next_block(
        &self,
        miner: impl Into<String>,
        transactions: Vec<Transaction>,
        workers: usize,
    ) -> Result<Block, ChainError> {
        self.mine_next_block_at_timestamp(
            miner.into(),
            transactions,
            workers,
            current_timestamp_secs(),
        )
    }

    pub fn add_block(&mut self, block: Block) -> Result<(), ChainError> {
        let ledger = self.validate_and_build_ledger()?;
        validate_block_against_chain(self, self.tip(), &ledger, &block)?;
        self.blocks.push(block);
        Ok(())
    }

    pub fn append_mined_block(
        &mut self,
        miner: impl Into<String>,
        transactions: Vec<Transaction>,
        workers: usize,
    ) -> Result<&Block, ChainError> {
        let block = self.mine_next_block(miner, transactions, workers)?;
        self.add_block(block)?;
        Ok(self.tip())
    }

    pub fn validate(&self) -> Result<(), ChainError> {
        self.validate_and_build_ledger().map(|_| ())
    }

    fn validate_and_build_ledger(&self) -> Result<HashMap<String, u64>, ChainError> {
        let Some((genesis, rest)) = self.blocks.split_first() else {
            return Err(ChainError::InvalidBlock {
                index: 0,
                reason: "chain is empty",
            });
        };

        validate_genesis(genesis)?;
        let mut previous = genesis;
        let mut ledger = HashMap::new();

        for block in rest {
            validate_block_against_chain(self, previous, &ledger, block)?;
            apply_transactions_to_ledger(block.index, &block.transactions, &mut ledger)?;
            previous = block;
        }

        Ok(ledger)
    }

    fn mine_next_block_at_timestamp(
        &self,
        miner: String,
        transactions: Vec<Transaction>,
        workers: usize,
        timestamp_secs: u64,
    ) -> Result<Block, ChainError> {
        if workers == 0 {
            return Err(ChainError::NoWorkers);
        }

        let index = self.tip().index + 1;
        let prev_hash = self.tip().hash;
        let transactions = Arc::new(self.prepare_block_transactions(index, miner, transactions));
        let merkle_root = calculate_merkle_root(transactions.as_ref());
        let difficulty_bits = self.difficulty_bits;
        let found = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let ledger = self.validate_and_build_ledger()?;

        validate_block_transactions(index, transactions.as_ref(), self.block_reward(index))?;
        let mut prospective_ledger = ledger.clone();
        apply_transactions_to_ledger(index, transactions.as_ref(), &mut prospective_ledger)?;

        thread::scope(|scope| {
            for worker_id in 0..workers {
                let found = Arc::clone(&found);
                let transactions = Arc::clone(&transactions);
                let tx = tx.clone();

                scope.spawn(move || {
                    let mut nonce = worker_id as u64;
                    while !found.load(Ordering::Acquire) {
                        let hash =
                            calculate_hash(index, &prev_hash, &merkle_root, timestamp_secs, nonce);
                        if meets_difficulty(&hash, difficulty_bits) {
                            if !found.swap(true, Ordering::AcqRel) {
                                let block = Block {
                                    index,
                                    prev_hash,
                                    merkle_root,
                                    timestamp_secs,
                                    nonce,
                                    hash,
                                    transactions: (*transactions).clone(),
                                };
                                let _ = tx.send(block);
                            }
                            break;
                        }

                        nonce = nonce.wrapping_add(workers as u64);
                    }
                });
            }

            drop(tx);
            rx.recv().map_err(|_| ChainError::MiningFailed)
        })
    }

    fn prepare_block_transactions(
        &self,
        block_index: u64,
        miner: String,
        transactions: Vec<Transaction>,
    ) -> Vec<Transaction> {
        let mut prepared = Vec::with_capacity(transactions.len() + 1);
        let reward = self.block_reward(block_index);
        if reward > 0 {
            prepared.push(Transaction::reward(miner, reward));
        }
        prepared.extend(transactions);
        prepared
    }
}

pub fn calculate_hash(
    index: u64,
    prev_hash: &Hash,
    merkle_root: &Hash,
    timestamp_secs: u64,
    nonce: u64,
) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(index.to_le_bytes());
    hasher.update(prev_hash);
    hasher.update(merkle_root);
    hasher.update(timestamp_secs.to_le_bytes());
    hasher.update(nonce.to_le_bytes());
    finalize_hash(hasher)
}

pub fn calculate_merkle_root(transactions: &[Transaction]) -> Hash {
    let mut level: Vec<Hash> = if transactions.is_empty() {
        vec![hash_bytes(&[])]
    } else {
        transactions
            .iter()
            .map(|transaction| hash_bytes(transaction.canonical_string().as_bytes()))
            .collect()
    };

    while level.len() > 1 {
        if level.len() % 2 == 1 {
            let last = *level.last().expect("level cannot be empty");
            level.push(last);
        }

        level = level
            .chunks(2)
            .map(|pair| hash_pair(&pair[0], &pair[1]))
            .collect();
    }

    level[0]
}

pub fn meets_difficulty(hash: &Hash, difficulty_bits: u32) -> bool {
    if difficulty_bits > 256 {
        return false;
    }

    let full_zero_bytes = (difficulty_bits / 8) as usize;
    let remaining_bits = (difficulty_bits % 8) as u8;

    if hash.iter().take(full_zero_bytes).any(|&byte| byte != 0) {
        return false;
    }

    if remaining_bits == 0 {
        return true;
    }

    hash[full_zero_bytes] >> (8 - remaining_bits) == 0
}

pub fn hash_to_hex(hash: &Hash) -> String {
    let mut out = String::with_capacity(hash.len() * 2);
    for byte in hash {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn validate_difficulty(difficulty_bits: u32) -> Result<(), ChainError> {
    if difficulty_bits <= 256 {
        Ok(())
    } else {
        Err(ChainError::InvalidDifficulty(difficulty_bits))
    }
}

fn validate_reward_schedule(initial_reward: u64, halving_interval: u64) -> Result<(), ChainError> {
    if halving_interval == 0 {
        Err(ChainError::InvalidRewardSchedule {
            initial_reward,
            halving_interval,
        })
    } else {
        Ok(())
    }
}

fn validate_genesis(block: &Block) -> Result<(), ChainError> {
    if block.index != 0 {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "genesis block must have index 0",
        });
    }

    if block.prev_hash != EMPTY_HASH {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "genesis block must point to the empty hash",
        });
    }

    if block.timestamp_secs != 0 {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "genesis block must use timestamp 0",
        });
    }

    if block.transactions != [Transaction::memo(GENESIS_MEMO)] {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "genesis block transactions are fixed",
        });
    }

    validate_block_contents(block)
}

fn validate_block_against_chain(
    blockchain: &Blockchain,
    previous: &Block,
    ledger: &HashMap<String, u64>,
    block: &Block,
) -> Result<(), ChainError> {
    validate_block_header(previous, block, blockchain.difficulty_bits)?;
    validate_block_transactions(
        block.index,
        &block.transactions,
        blockchain.block_reward(block.index),
    )?;

    let mut next_ledger = ledger.clone();
    apply_transactions_to_ledger(block.index, &block.transactions, &mut next_ledger)?;
    Ok(())
}

fn validate_block_header(
    previous: &Block,
    block: &Block,
    difficulty_bits: u32,
) -> Result<(), ChainError> {
    if block.index != previous.index + 1 {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "block index must increment by one",
        });
    }

    if block.prev_hash != previous.hash {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "previous hash does not match the current chain tip",
        });
    }

    if block.timestamp_secs < previous.timestamp_secs {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "block timestamp must not move backwards",
        });
    }

    if !meets_difficulty(&block.hash, difficulty_bits) {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "block hash does not satisfy chain difficulty",
        });
    }

    validate_block_contents(block)
}

fn validate_block_transactions(
    block_index: u64,
    transactions: &[Transaction],
    expected_reward: u64,
) -> Result<(), ChainError> {
    if expected_reward > 0 {
        match transactions.first() {
            Some(Transaction::Reward { to, amount })
                if !to.is_empty() && *amount == expected_reward => {}
            Some(Transaction::Reward { .. }) => {
                return Err(ChainError::InvalidBlock {
                    index: block_index,
                    reason: "block reward amount does not match the halving schedule",
                });
            }
            _ => {
                return Err(ChainError::InvalidBlock {
                    index: block_index,
                    reason: "first transaction must be the block reward",
                });
            }
        }
    } else if transactions
        .iter()
        .any(|transaction| matches!(transaction, Transaction::Reward { .. }))
    {
        return Err(ChainError::InvalidBlock {
            index: block_index,
            reason: "reward transactions are not allowed once rewards reach zero",
        });
    }

    for transaction in transactions {
        match transaction {
            Transaction::Reward { to, amount } => {
                if to.is_empty() {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "reward transactions require a miner address",
                    });
                }
                if *amount == 0 {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "reward transactions must have a non-zero amount",
                    });
                }
            }
            Transaction::Transfer { from, to, amount } => {
                if from.is_empty() || to.is_empty() {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "transfer participants must not be empty",
                    });
                }
                if from == to {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "transfer sender and recipient must differ",
                    });
                }
                if *amount == 0 {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "transfer amounts must be non-zero",
                    });
                }
            }
            Transaction::Memo(text) => {
                if text.trim().is_empty() {
                    return Err(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "memo transactions must not be blank",
                    });
                }
            }
        }
    }

    if transactions
        .iter()
        .skip(1)
        .any(|transaction| matches!(transaction, Transaction::Reward { .. }))
    {
        return Err(ChainError::InvalidBlock {
            index: block_index,
            reason: "block rewards must appear before all other transactions",
        });
    }

    Ok(())
}

fn validate_block_contents(block: &Block) -> Result<(), ChainError> {
    let expected_merkle_root = calculate_merkle_root(&block.transactions);
    if block.merkle_root != expected_merkle_root {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "merkle root does not match the block transactions",
        });
    }

    let expected_hash = calculate_hash(
        block.index,
        &block.prev_hash,
        &block.merkle_root,
        block.timestamp_secs,
        block.nonce,
    );
    if block.hash != expected_hash {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "block hash does not match the block contents",
        });
    }

    Ok(())
}

fn apply_transactions_to_ledger(
    block_index: u64,
    transactions: &[Transaction],
    ledger: &mut HashMap<String, u64>,
) -> Result<(), ChainError> {
    for transaction in transactions {
        match transaction {
            Transaction::Reward { to, amount } => {
                let balance = ledger.entry(to.clone()).or_default();
                *balance = balance
                    .checked_add(*amount)
                    .ok_or(ChainError::InvalidBlock {
                        index: block_index,
                        reason: "account balance overflowed",
                    })?;
            }
            Transaction::Transfer { from, to, amount } => {
                let available = ledger.get(from).copied().unwrap_or(0);
                if available < *amount {
                    return Err(ChainError::InsufficientFunds {
                        block_index,
                        account: from.clone(),
                        available,
                        required: *amount,
                    });
                }

                ledger.insert(from.clone(), available - amount);
                let recipient_balance = ledger.entry(to.clone()).or_default();
                *recipient_balance =
                    recipient_balance
                        .checked_add(*amount)
                        .ok_or(ChainError::InvalidBlock {
                            index: block_index,
                            reason: "account balance overflowed",
                        })?;
            }
            Transaction::Memo(_) => {}
        }
    }

    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    finalize_hash(hasher)
}

fn hash_pair(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    finalize_hash(hasher)
}

fn finalize_hash(hasher: Sha256) -> Hash {
    let out = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&out);
    hash
}

fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_partial_byte_difficulty() {
        let hash = [
            0x00, 0x0f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff,
        ];

        assert!(meets_difficulty(&hash, 12));
        assert!(!meets_difficulty(&hash, 13));
    }

    #[test]
    fn rejects_zero_workers() {
        let chain = Blockchain::new(4).unwrap();
        let error = chain
            .mine_next_block("alice", vec![Transaction::memo("warmup")], 0)
            .unwrap_err();
        assert_eq!(error, ChainError::NoWorkers);
    }

    #[test]
    fn appends_valid_blocks_tracks_balances_and_validates_chain() {
        let mut chain = Blockchain::with_consensus(8, 50, 2).unwrap();
        chain
            .append_mined_block("alice", vec![Transaction::memo("boot")], 2)
            .unwrap();
        chain
            .append_mined_block("bob", vec![Transaction::transfer("alice", "bob", 20)], 2)
            .unwrap();
        chain
            .append_mined_block("alice", vec![Transaction::transfer("bob", "carol", 10)], 2)
            .unwrap();

        assert_eq!(chain.block_reward(1), 50);
        assert_eq!(chain.block_reward(3), 25);
        assert_eq!(chain.balance_of("alice").unwrap(), 55);
        assert_eq!(chain.balance_of("bob").unwrap(), 60);
        assert_eq!(chain.balance_of("carol").unwrap(), 10);
        assert!(chain.validate().is_ok());
    }

    #[test]
    fn rejects_overspending_transfers() {
        let chain = Blockchain::new(4).unwrap();
        let error = chain
            .mine_next_block("bob", vec![Transaction::transfer("alice", "bob", 1)], 1)
            .unwrap_err();

        assert_eq!(
            error,
            ChainError::InsufficientFunds {
                block_index: 1,
                account: "alice".to_owned(),
                available: 0,
                required: 1,
            }
        );
    }

    #[test]
    fn rejects_wrong_reward_amount() {
        let mut chain = Blockchain::with_consensus(0, 50, 2).unwrap();
        let mut block = chain
            .mine_next_block_at_timestamp("alice".to_owned(), vec![Transaction::memo("boot")], 1, 1)
            .unwrap();

        block.transactions[0] = Transaction::reward("alice", 99);
        block.merkle_root = calculate_merkle_root(&block.transactions);
        block.hash = calculate_hash(
            block.index,
            &block.prev_hash,
            &block.merkle_root,
            block.timestamp_secs,
            block.nonce,
        );

        let error = chain.add_block(block).unwrap_err();
        assert_eq!(
            error,
            ChainError::InvalidBlock {
                index: 1,
                reason: "block reward amount does not match the halving schedule",
            }
        );
    }

    #[test]
    fn rejects_backwards_timestamps() {
        let mut chain = Blockchain::with_consensus(0, 50, 10).unwrap();
        let first = chain
            .mine_next_block_at_timestamp(
                "alice".to_owned(),
                vec![Transaction::memo("boot")],
                1,
                10,
            )
            .unwrap();
        chain.add_block(first).unwrap();

        let mut second = chain
            .mine_next_block_at_timestamp(
                "bob".to_owned(),
                vec![Transaction::transfer("alice", "bob", 5)],
                1,
                11,
            )
            .unwrap();
        second.timestamp_secs = 9;
        second.hash = calculate_hash(
            second.index,
            &second.prev_hash,
            &second.merkle_root,
            second.timestamp_secs,
            second.nonce,
        );

        let error = chain.add_block(second).unwrap_err();
        assert_eq!(
            error,
            ChainError::InvalidBlock {
                index: 2,
                reason: "block timestamp must not move backwards",
            }
        );
    }

    #[test]
    fn rejects_tampered_blocks() {
        let mut chain = Blockchain::with_consensus(4, 50, 10).unwrap();
        chain
            .append_mined_block("alice", vec![Transaction::memo("boot")], 2)
            .unwrap();

        let mut block = chain
            .mine_next_block("bob", vec![Transaction::transfer("alice", "bob", 5)], 2)
            .unwrap();
        block.transactions.push(Transaction::memo("tamper"));

        let error = chain.add_block(block).unwrap_err();
        assert_eq!(
            error,
            ChainError::InvalidBlock {
                index: 2,
                reason: "merkle root does not match the block transactions",
            }
        );
    }

    #[test]
    fn merkle_root_changes_with_transaction_order() {
        let first = calculate_merkle_root(&[
            Transaction::memo("a"),
            Transaction::transfer("alice", "bob", 1),
        ]);
        let second = calculate_merkle_root(&[
            Transaction::transfer("alice", "bob", 1),
            Transaction::memo("a"),
        ]);
        assert_ne!(first, second);
    }

    #[test]
    fn reports_chain_stats_and_top_accounts() {
        let mut chain = Blockchain::with_consensus(0, 50, 2).unwrap();
        chain
            .append_mined_block("alice", vec![Transaction::memo("boot")], 1)
            .unwrap();
        chain
            .append_mined_block("bob", vec![Transaction::transfer("alice", "bob", 20)], 1)
            .unwrap();

        let stats = chain.stats().unwrap();
        assert_eq!(stats.height, 2);
        assert_eq!(stats.total_blocks, 3);
        assert_eq!(stats.total_transactions, 5);
        assert_eq!(stats.reward_transactions, 2);
        assert_eq!(stats.transfer_transactions, 1);
        assert_eq!(stats.memo_transactions, 2);
        assert_eq!(stats.circulating_supply, 100);
        assert_eq!(stats.unique_accounts, 2);
        assert_eq!(stats.richest_account, Some("bob".to_owned()));
        assert_eq!(stats.richest_balance, 70);
        assert_eq!(stats.next_block_reward, 25);

        assert_eq!(
            chain.top_accounts(2).unwrap(),
            vec![("bob".to_owned(), 70), ("alice".to_owned(), 30)]
        );
    }

    #[test]
    fn builds_account_statements() {
        let mut chain = Blockchain::with_consensus(0, 50, 2).unwrap();
        chain
            .append_mined_block("alice", vec![Transaction::memo("boot")], 1)
            .unwrap();
        chain
            .append_mined_block("bob", vec![Transaction::transfer("alice", "bob", 20)], 1)
            .unwrap();

        let statement = chain.account_statement("bob").unwrap();
        assert_eq!(statement.balance, 70);
        assert_eq!(statement.mined_rewards, 50);
        assert_eq!(statement.transfers_sent, 0);
        assert_eq!(statement.transfers_received, 20);
        assert_eq!(
            statement.activity,
            vec![
                AccountActivity {
                    block_index: 2,
                    kind: AccountActivityKind::Reward,
                    counterparty: None,
                    amount: 50,
                },
                AccountActivity {
                    block_index: 2,
                    kind: AccountActivityKind::Received,
                    counterparty: Some("alice".to_owned()),
                    amount: 20,
                },
            ]
        );
    }
}
