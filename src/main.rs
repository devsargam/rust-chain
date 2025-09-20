use rand::{Rng, SeedableRng, rngs::StdRng};
use rust_chain::{AccountActivityKind, Blockchain, Transaction, hash_to_hex};
use std::{cmp, env, error::Error, io, thread};

const DEFAULT_CHAIN_LENGTH: usize = 10;
const DEFAULT_DIFFICULTY_BITS: u32 = 12;
const DEFAULT_SEED: u64 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimulationConfig {
    chain_length: usize,
    difficulty_bits: u32,
    workers: usize,
    seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Demo(SimulationConfig),
    Summary(SimulationConfig),
    Balances(SimulationConfig),
    Account {
        account: String,
        config: SimulationConfig,
    },
    Block {
        index: u64,
        config: SimulationConfig,
    },
    Help,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command =
        parse_command(&args).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

    match command {
        Command::Demo(config) => run_demo(config)?,
        Command::Summary(config) => {
            let chain = build_demo_chain(config, false)?;
            print_summary(&chain, config)?;
        }
        Command::Balances(config) => {
            let chain = build_demo_chain(config, false)?;
            print_balances(&chain)?;
        }
        Command::Account { account, config } => {
            let chain = build_demo_chain(config, false)?;
            print_account(&chain, &account)?;
        }
        Command::Block { index, config } => {
            let chain = build_demo_chain(config, false)?;
            print_block(&chain, index)?;
        }
        Command::Help => print_usage(),
    }

    Ok(())
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args.first().map(String::as_str) {
        None => Ok(Command::Demo(default_config())),
        Some("help" | "--help" | "-h") => Ok(Command::Help),
        Some("demo") => Ok(Command::Demo(parse_config(&args[1..])?)),
        Some("summary") => Ok(Command::Summary(parse_config(&args[1..])?)),
        Some("balances") => Ok(Command::Balances(parse_config(&args[1..])?)),
        Some("account") => {
            let account = args.get(1).ok_or_else(|| {
                "usage: cargo run -- account <name> [chain_length] [difficulty_bits] [workers] [seed]"
                    .to_owned()
            })?;
            Ok(Command::Account {
                account: account.clone(),
                config: parse_config(&args[2..])?,
            })
        }
        Some("block") => {
            let index = parse_required(args.get(1), "block_index")?;
            Ok(Command::Block {
                index,
                config: parse_config(&args[2..])?,
            })
        }
        Some(value) if value.parse::<usize>().is_ok() => Ok(Command::Demo(parse_config(args)?)),
        Some(value) => Err(format!(
            "unknown command `{value}`. Run `cargo run -- help` for usage."
        )),
    }
}

fn parse_config(args: &[String]) -> Result<SimulationConfig, String> {
    let mut config = default_config();

    if let Some(value) = args.first() {
        config.chain_length = parse_value(value, "chain_length")?;
    }
    if let Some(value) = args.get(1) {
        config.difficulty_bits = parse_value(value, "difficulty_bits")?;
    }
    if let Some(value) = args.get(2) {
        config.workers = parse_value(value, "workers")?;
    }
    if let Some(value) = args.get(3) {
        config.seed = parse_value(value, "seed")?;
    }

    Ok(config)
}

fn parse_required<T>(value: Option<&String>, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    let Some(value) = value else {
        return Err(format!("missing required argument `{label}`"));
    };

    parse_value(value, label)
}

fn parse_value<T>(value: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("invalid {label}: `{value}`"))
}

fn default_config() -> SimulationConfig {
    SimulationConfig {
        chain_length: DEFAULT_CHAIN_LENGTH,
        difficulty_bits: DEFAULT_DIFFICULTY_BITS,
        workers: default_workers(),
        seed: DEFAULT_SEED,
    }
}

fn default_workers() -> usize {
    thread::available_parallelism().map_or(4, |parallelism| parallelism.get())
}

fn run_demo(config: SimulationConfig) -> Result<(), Box<dyn Error>> {
    let chain = build_demo_chain(config, true)?;
    println!("\nChain valid at height {}", chain.len() - 1);
    println!("Balances:");
    for (account, balance) in sorted_balances(&chain)? {
        println!("  {account}: {balance}");
    }

    Ok(())
}

fn build_demo_chain(config: SimulationConfig, verbose: bool) -> Result<Blockchain, Box<dyn Error>> {
    let mut chain = Blockchain::new(config.difficulty_bits)?;
    let accounts = ["alice", "bob", "carol", "dave"];
    let mut rng = StdRng::seed_from_u64(config.seed);

    if verbose {
        println!(
            "Mining {} blocks with {} workers at difficulty {} bits",
            config.chain_length.saturating_sub(1),
            config.workers,
            chain.difficulty_bits()
        );
        println!(
            "Reward schedule: start={} halving_interval={} blocks",
            chain.initial_reward(),
            chain.halving_interval()
        );
        println!("Seed: {}", config.seed);
    }

    for height in 1..config.chain_length {
        let miner = accounts[rng.random_range(0..accounts.len())];
        let reward = chain.block_reward(height as u64);
        let transactions = generate_transactions(&chain, &accounts, miner, height, &mut rng)?;
        let user_transaction_count = transactions.len();

        if verbose {
            println!(
                "\nMining block {height} for {miner} with reward {reward} and {user_transaction_count} user transactions"
            );
        }

        let block = chain.append_mined_block(miner, transactions, config.workers)?;
        if verbose {
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
    }

    chain.validate()?;
    Ok(chain)
}

fn print_summary(chain: &Blockchain, config: SimulationConfig) -> Result<(), Box<dyn Error>> {
    let stats = chain.stats()?;
    println!("Chain summary");
    println!("  seed: {}", config.seed);
    println!("  height: {}", stats.height);
    println!("  blocks: {}", stats.total_blocks);
    println!("  difficulty_bits: {}", stats.difficulty_bits);
    println!("  transactions: {}", stats.total_transactions);
    println!("  rewards: {}", stats.reward_transactions);
    println!("  transfers: {}", stats.transfer_transactions);
    println!("  memos: {}", stats.memo_transactions);
    println!("  circulating_supply: {}", stats.circulating_supply);
    println!("  unique_accounts: {}", stats.unique_accounts);
    println!("  next_block_reward: {}", stats.next_block_reward);
    if let Some(account) = stats.richest_account {
        println!("  richest_account: {account} ({})", stats.richest_balance);
    }

    let top_accounts = chain.top_accounts(5)?;
    if !top_accounts.is_empty() {
        println!("Top accounts:");
        for (account, balance) in top_accounts {
            println!("  {account}: {balance}");
        }
    }

    Ok(())
}

fn print_balances(chain: &Blockchain) -> Result<(), Box<dyn Error>> {
    println!("Balances");
    for (account, balance) in chain.top_accounts(usize::MAX)? {
        println!("  {account}: {balance}");
    }

    Ok(())
}

fn print_account(chain: &Blockchain, account: &str) -> Result<(), Box<dyn Error>> {
    let statement = chain.account_statement(account)?;
    let total_inflow = statement.mined_rewards + statement.transfers_received;
    let net_flow = i128::from(total_inflow) - i128::from(statement.transfers_sent);

    println!("Account {account}");
    println!("  balance: {}", statement.balance);
    println!("  mined_rewards: {}", statement.mined_rewards);
    println!("  transfers_received: {}", statement.transfers_received);
    println!("  transfers_sent: {}", statement.transfers_sent);
    println!("  net_flow: {net_flow}");

    if statement.activity.is_empty() {
        println!("  no reward or transfer activity recorded");
        return Ok(());
    }

    println!("Activity:");
    for entry in statement.activity {
        match entry.kind {
            AccountActivityKind::Reward => {
                println!("  block {} reward {}", entry.block_index, entry.amount);
            }
            AccountActivityKind::Sent => {
                let counterparty = entry.counterparty.unwrap_or_else(|| "unknown".to_owned());
                println!(
                    "  block {} sent {} to {}",
                    entry.block_index, entry.amount, counterparty
                );
            }
            AccountActivityKind::Received => {
                let counterparty = entry.counterparty.unwrap_or_else(|| "unknown".to_owned());
                println!(
                    "  block {} received {} from {}",
                    entry.block_index, entry.amount, counterparty
                );
            }
        }
    }

    Ok(())
}

fn print_block(chain: &Blockchain, index: u64) -> Result<(), Box<dyn Error>> {
    let block = chain.block(index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("block {index} was not found"),
        )
    })?;

    println!("Block {}", block.index);
    println!("  timestamp: {}", block.timestamp_secs);
    println!("  nonce: {}", block.nonce);
    println!("  previous_hash: {}", hash_to_hex(&block.prev_hash));
    println!("  merkle_root: {}", hash_to_hex(&block.merkle_root));
    println!("  hash: {}", hash_to_hex(&block.hash));
    println!("  transactions: {}", block.transactions.len());

    for transaction in &block.transactions {
        println!("  - {transaction}");
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

fn print_usage() {
    println!("rust-chain explorer");
    println!();
    println!("Commands:");
    println!("  cargo run -- demo [chain_length] [difficulty_bits] [workers] [seed]");
    println!("  cargo run -- summary [chain_length] [difficulty_bits] [workers] [seed]");
    println!("  cargo run -- balances [chain_length] [difficulty_bits] [workers] [seed]");
    println!(
        "  cargo run -- block <block_index> [chain_length] [difficulty_bits] [workers] [seed]"
    );
    println!("  cargo run -- account <name> [chain_length] [difficulty_bits] [workers] [seed]");
    println!();
    println!("Legacy demo usage still works:");
    println!("  cargo run -- [chain_length] [difficulty_bits] [workers] [seed]");
    println!();
    println!(
        "Defaults: chain_length={DEFAULT_CHAIN_LENGTH} difficulty_bits={DEFAULT_DIFFICULTY_BITS} workers={} seed={DEFAULT_SEED}",
        default_workers()
    );
}
