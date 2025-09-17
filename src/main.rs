use rand::Rng;
use rust_chain::{Blockchain, Transaction, hash_to_hex};
use std::{cmp, env, error::Error, thread};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let chain_length = parse_arg(args.get(1), 10usize);
    let difficulty_bits = parse_arg(args.get(2), 12u32);
    let workers = parse_arg(
        args.get(3),
        thread::available_parallelism().map_or(4, |parallelism| parallelism.get()),
    );

    let mut chain = Blockchain::new(difficulty_bits)?;
    let accounts = ["alice", "bob", "carol", "dave"];
    let mut rng = rand::rng();

    println!(
        "Mining {} blocks with {} workers at difficulty {} bits",
        chain_length.saturating_sub(1),
        workers,
        chain.difficulty_bits()
    );
    println!(
        "Reward schedule: start={} halving_interval={} blocks",
        chain.initial_reward(),
        chain.halving_interval()
    );

    for height in 1..chain_length {
        let miner = accounts[rng.random_range(0..accounts.len())];
        let reward = chain.block_reward(height as u64);
        let transactions = generate_transactions(&chain, &accounts, miner, height, &mut rng)?;

        println!(
            "\nMining block {height} for {miner} with reward {reward} and {} user transactions",
            transactions.len()
        );

        let block = chain.append_mined_block(miner, transactions, workers)?;
        println!(
            "Mined block {} ts={} nonce={} hash={}",
            block.index,
            block.timestamp_secs,
            block.nonce,
            hash_to_hex(&block.hash)
        );

        for transaction in &block.transactions {
            println!("  - {transaction}");
        }
    }

    chain.validate()?;
    println!("\nChain valid at height {}", chain.len() - 1);
    println!("Balances:");
    for (account, balance) in sorted_balances(&chain)? {
        println!("  {account}: {balance}");
    }

    Ok(())
}

fn generate_transactions<R: Rng + ?Sized>(
    chain: &Blockchain,
    accounts: &[&str],
    miner: &str,
    height: usize,
    rng: &mut R,
) -> Result<Vec<Transaction>, Box<dyn Error>> {
    let mut preview_balances = chain.balances()?;
    let reward = chain.block_reward(height as u64);
    if reward > 0 {
        *preview_balances.entry(miner.to_owned()).or_default() += reward;
    }

    let mut transactions = Vec::new();
    let transfer_budget = rng.random_range(0..=2);

    for _ in 0..transfer_budget {
        let funded_accounts: Vec<(String, u64)> = preview_balances
            .iter()
            .filter_map(|(account, balance)| {
                if *balance > 0 {
                    Some((account.clone(), *balance))
                } else {
                    None
                }
            })
            .collect();

        if funded_accounts.is_empty() {
            break;
        }

        let (from, available) = funded_accounts[rng.random_range(0..funded_accounts.len())].clone();
        let recipients: Vec<&str> = accounts
            .iter()
            .copied()
            .filter(|account| *account != from)
            .collect();
        if recipients.is_empty() {
            break;
        }

        let to = recipients[rng.random_range(0..recipients.len())];
        let amount = rng.random_range(1..=cmp::min(available, 25));
        transactions.push(Transaction::transfer(from.clone(), to, amount));

        *preview_balances.get_mut(&from).expect("sender must exist") -= amount;
        *preview_balances.entry(to.to_owned()).or_default() += amount;
    }

    if transactions.is_empty() || rng.random_bool(0.45) {
        transactions.push(Transaction::memo(format!(
            "block {height} says hi from {miner}"
        )));
    }

    Ok(transactions)
}

fn sorted_balances(chain: &Blockchain) -> Result<Vec<(String, u64)>, Box<dyn Error>> {
    let mut balances: Vec<_> = chain.balances()?.into_iter().collect();
    balances.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(balances)
}

fn parse_arg<T>(value: Option<&String>, default: T) -> T
where
    T: Copy + std::str::FromStr,
{
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
