use sha2::{Digest, Sha256};
use std::fmt::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;

pub type Hash = [u8; 32];

const EMPTY_HASH: Hash = [0; 32];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub index: u64,
    pub prev_hash: Hash,
    pub merkle_root: Hash,
    pub nonce: u64,
    pub hash: Hash,
    pub transactions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blockchain {
    blocks: Vec<Block>,
    difficulty_bits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    InvalidDifficulty(u32),
    NoWorkers,
    InvalidBlock { index: u64, reason: &'static str },
    MiningFailed,
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDifficulty(bits) => {
                write!(f, "difficulty must be between 0 and 256 bits, got {bits}")
            }
            Self::NoWorkers => f.write_str("mining requires at least one worker"),
            Self::InvalidBlock { index, reason } => {
                write!(f, "block {index} is invalid: {reason}")
            }
            Self::MiningFailed => f.write_str("mining finished without producing a block"),
        }
    }
}

impl std::error::Error for ChainError {}

impl Blockchain {
    pub fn new(difficulty_bits: u32) -> Result<Self, ChainError> {
        validate_difficulty(difficulty_bits)?;

        let genesis_transactions = vec!["genesis".to_owned()];
        let genesis_merkle_root = calculate_merkle_root(&genesis_transactions);
        let genesis = Block {
            index: 0,
            prev_hash: EMPTY_HASH,
            merkle_root: genesis_merkle_root,
            nonce: 0,
            hash: calculate_hash(0, &EMPTY_HASH, &genesis_merkle_root, 0),
            transactions: genesis_transactions,
        };

        Ok(Self {
            blocks: vec![genesis],
            difficulty_bits,
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

    pub fn tip(&self) -> &Block {
        self.blocks
            .last()
            .expect("blockchain is always initialized with a genesis block")
    }

    pub fn mine_next_block(
        &self,
        transactions: Vec<String>,
        workers: usize,
    ) -> Result<Block, ChainError> {
        if workers == 0 {
            return Err(ChainError::NoWorkers);
        }

        let index = self.tip().index + 1;
        let prev_hash = self.tip().hash;
        let merkle_root = calculate_merkle_root(&transactions);
        let difficulty_bits = self.difficulty_bits;
        let found = Arc::new(AtomicBool::new(false));
        let transactions = Arc::new(transactions);
        let (tx, rx) = mpsc::channel();

        thread::scope(|scope| {
            for worker_id in 0..workers {
                let found = Arc::clone(&found);
                let transactions = Arc::clone(&transactions);
                let tx = tx.clone();

                scope.spawn(move || {
                    let mut nonce = worker_id as u64;
                    while !found.load(Ordering::Acquire) {
                        let hash = calculate_hash(index, &prev_hash, &merkle_root, nonce);
                        if meets_difficulty(&hash, difficulty_bits) {
                            if !found.swap(true, Ordering::AcqRel) {
                                let block = Block {
                                    index,
                                    prev_hash,
                                    merkle_root,
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

    pub fn add_block(&mut self, block: Block) -> Result<(), ChainError> {
        validate_block_against_previous(self.tip(), &block, self.difficulty_bits)?;
        self.blocks.push(block);
        Ok(())
    }

    pub fn append_mined_block(
        &mut self,
        transactions: Vec<String>,
        workers: usize,
    ) -> Result<&Block, ChainError> {
        let block = self.mine_next_block(transactions, workers)?;
        self.add_block(block)?;
        Ok(self.tip())
    }

    pub fn validate(&self) -> Result<(), ChainError> {
        let Some((genesis, rest)) = self.blocks.split_first() else {
            return Err(ChainError::InvalidBlock {
                index: 0,
                reason: "chain is empty",
            });
        };

        validate_genesis(genesis)?;
        let mut previous = genesis;
        for block in rest {
            validate_block_against_previous(previous, block, self.difficulty_bits)?;
            previous = block;
        }

        Ok(())
    }
}

pub fn calculate_hash(index: u64, prev_hash: &Hash, merkle_root: &Hash, nonce: u64) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(index.to_le_bytes());
    hasher.update(prev_hash);
    hasher.update(merkle_root);
    hasher.update(nonce.to_le_bytes());
    finalize_hash(hasher)
}

pub fn calculate_merkle_root(transactions: &[String]) -> Hash {
    let mut level: Vec<Hash> = if transactions.is_empty() {
        vec![hash_bytes(&[])]
    } else {
        transactions
            .iter()
            .map(|transaction| hash_bytes(transaction.as_bytes()))
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

    validate_block_contents(block)
}

fn validate_block_against_previous(
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

    if !meets_difficulty(&block.hash, difficulty_bits) {
        return Err(ChainError::InvalidBlock {
            index: block.index,
            reason: "block hash does not satisfy chain difficulty",
        });
    }

    validate_block_contents(block)
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
        let error = chain.mine_next_block(vec!["tx".into()], 0).unwrap_err();
        assert_eq!(error, ChainError::NoWorkers);
    }

    #[test]
    fn appends_valid_blocks_and_validates_the_chain() {
        let mut chain = Blockchain::new(8).unwrap();
        chain
            .append_mined_block(vec!["alice->bob:5".into(), "bob->carol:1".into()], 2)
            .unwrap();
        chain
            .append_mined_block(vec!["carol->dave:1".into()], 2)
            .unwrap();

        assert_eq!(chain.len(), 3);
        assert!(chain.validate().is_ok());
    }

    #[test]
    fn rejects_tampered_blocks() {
        let mut chain = Blockchain::new(4).unwrap();
        let mut block = chain
            .mine_next_block(vec!["alice->bob:5".into()], 2)
            .unwrap();
        block.transactions.push("mallory->mallory:99".into());

        let error = chain.add_block(block).unwrap_err();
        assert_eq!(
            error,
            ChainError::InvalidBlock {
                index: 1,
                reason: "merkle root does not match the block transactions",
            }
        );
    }

    #[test]
    fn merkle_root_changes_with_transaction_order() {
        let first = calculate_merkle_root(&["a".to_owned(), "b".to_owned()]);
        let second = calculate_merkle_root(&["b".to_owned(), "a".to_owned()]);
        assert_ne!(first, second);
    }
}
