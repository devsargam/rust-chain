use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;

#[derive(Clone)]
struct BlockHeader {
    prev_hash: [u8; 32],
    merkle_root: [u8; 32],
    nonce: u64,
}

fn hash_header(header: &BlockHeader) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(&header.prev_hash);
    hasher.update(&header.merkle_root);
    hasher.update(&header.nonce.to_le_bytes());
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn meets_target(hash: &[u8; 32], difficulty_zero_bytes: usize) -> bool {
    hash.iter().take(difficulty_zero_bytes).all(|&b| b == 0)
}

fn main() {
    let workers = 10;
    let difficulty = 3;

    let mut rng = rand::rng();

    let header = BlockHeader {
        prev_hash: rng.random(),
        merkle_root: rng.random(),
        nonce: 0,
    };

    let found = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None));

    let (tx, rx) = mpsc::channel();

    for i in 0..workers {
        let mut local_header = header.clone();
        let found = found.clone();
        let result = result.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            let mut nonce = i as u64;
            while !found.load(Ordering::Relaxed) {
                local_header.nonce = nonce;
                let h = hash_header(&local_header);
                if meets_target(&h, difficulty) {
                    found.store(true, Ordering::Relaxed);
                    *result.lock().unwrap() = Some((nonce, h));
                    tx.send((i, nonce)).unwrap();
                    break;
                }
                nonce += workers as u64;
            }
        });
    }

    let (worker_id, nonce) = rx.recv().unwrap();
    println!("Worker {worker_id} found valid nonce: {nonce}");
    let res = result.lock().unwrap();
    println!("Hash: {:x?}", res.as_ref().unwrap().1);
}
