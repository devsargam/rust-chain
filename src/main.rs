use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;

#[derive(Debug, Clone)]
struct Block {
    index: u64,
    prev_hash: [u8; 32],
    merkle_root: [u8; 32],
    nonce: u64,
    hash: [u8; 32],
}

fn meets_target(hash: &[u8; 32], difficulty_zero_bytes: usize) -> bool {
    hash.iter().take(difficulty_zero_bytes).all(|&b| b == 0)
}

fn calculate_hash(
    index: u64,
    prev_hash: &[u8; 32],
    merkle_root: &[u8; 32],
    nonce: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&index.to_le_bytes());
    hasher.update(prev_hash);
    hasher.update(merkle_root);
    hasher.update(&nonce.to_le_bytes());
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn mine_blocks(index: u64, prev_hash: [u8; 32], difficulty: usize, workers: usize) -> Block {
    let found = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None));

    let (tx, rx) = mpsc::channel();

    let merkle_root: [u8; 32] = rand::random();

    for i in 0..workers {
        let found = found.clone();
        let result = result.clone();
        let tx = tx.clone();
        let prev_hash = prev_hash.clone();
        let merkle_root = merkle_root.clone();

        thread::spawn(move || {
            let mut nonce = i as u64;
            while !found.load(Ordering::Relaxed) {
                let hash = calculate_hash(index, &prev_hash, &merkle_root, nonce);
                if meets_target(&hash, difficulty) {
                    found.store(true, Ordering::Relaxed);
                    let block = Block {
                        index,
                        prev_hash,
                        merkle_root,
                        nonce,
                        hash,
                    };
                    *result.lock().unwrap() = Some(block.clone());
                    tx.send(block).unwrap();
                    break;
                }
                nonce += workers as u64;
            }
        });
    }

    rx.recv().unwrap()
}

fn main() {
    let workers = 10;
    let difficulty = 3;
    let chain_max_length = 10;
    let mut chain: Vec<Block> = Vec::new();

    let mut rng = rand::rng();

    // Create the initial genesis
    let genesis = Block {
        prev_hash: rng.random(),
        merkle_root: rng.random(),
        nonce: 0,
        hash: rng.random(),
        index: 0,
    };
    chain.push(genesis);

    println!("Starting mining with {} workers...", workers);
    for height in 1..chain_max_length {
        let prev_hash = chain.last().unwrap().hash;
        println!("\n⛏️  Mining block {}", height);

        let block = mine_blocks(height, prev_hash, difficulty, workers);
        println!(
            "✅ Block {} mined! Nonce: {}, Hash: {:x?}",
            block.index, block.nonce, block.hash
        );

        chain.push(block);
    }
}
