# rust-chain

A small proof-of-work blockchain in Rust with deterministic block construction, ledger validation, concurrent mining, and explorer-style CLI commands.

## Features

- Proof-of-work blocks with configurable difficulty.
- Deterministic block transaction hashing via a Merkle root.
- Reward halving schedule with validation against tampered rewards.
- Ledger-based transfer validation that rejects overspending.
- Concurrent mining workers for block search.
- Seeded simulation runs so the same command can be reproduced exactly.
- Explorer commands for chain summaries, balances, block inspection, and account history.

## Running

```bash
cargo run -- demo [chain_length] [difficulty_bits] [workers] [seed]
```

Legacy positional demo usage still works:

```bash
cargo run -- [chain_length] [difficulty_bits] [workers] [seed]
```

## Explorer Commands

Summarize the chain:

```bash
cargo run -- summary 12 12 4 7
```

Show ranked balances:

```bash
cargo run -- balances 12 12 4 7
```

Inspect one block:

```bash
cargo run -- block 4 12 12 4 7
```

Inspect one account:

```bash
cargo run -- account alice 12 12 4 7
```

Run `cargo run -- help` to print the full command list and defaults.

## Testing

```bash
cargo test
```

## Good Next Additions

- Persist chains to disk and reload them between runs.
- Add signed transactions instead of trusting plain account names.
- Separate a mempool from mined blocks.
- Add peer-to-peer synchronization between nodes.
- Export blocks and balances as JSON for external tooling.
