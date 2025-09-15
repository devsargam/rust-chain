use rand::Rng;
use rust_chain::{Blockchain, hash_to_hex};
use std::{env, error::Error, thread};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let chain_length = parse_arg(args.get(1), 10usize);
    let difficulty_bits = parse_arg(args.get(2), 12u32);
    let workers = parse_arg(
        args.get(3),
        thread::available_parallelism().map_or(4, |parallelism| parallelism.get()),
    );

    let mut chain = Blockchain::new(difficulty_bits)?;
    let mut rng = rand::rng();

    println!(
        "Mining {} blocks with {} workers at difficulty {} bits",
        chain_length.saturating_sub(1),
        workers,
        chain.difficulty_bits()
    );

    for height in 1..chain_length {
        let transaction_count = rng.random_range(1..=4);
        let transactions: Vec<String> = (0..transaction_count)
            .map(|offset| format!("tx:{height}:{offset}:{}", rng.random::<u64>()))
            .collect();

        println!("\nMining block {height} with {transaction_count} transactions");

        let block = chain.append_mined_block(transactions, workers)?;
        println!(
            "Mined block {} nonce={} hash={}",
            block.index,
            block.nonce,
            hash_to_hex(&block.hash)
        );
    }

    chain.validate()?;
    println!("\nChain valid at height {}", chain.len() - 1);

    Ok(())
}

fn parse_arg<T>(value: Option<&String>, default: T) -> T
where
    T: Copy + std::str::FromStr,
{
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
