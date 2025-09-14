# rust-chain

A simple blockchain implementation in Rust.

## How it Works

This program demonstrates a basic proof-of-work blockchain. It begins with an initial "genesis" block and then continuously mines new blocks to add to the chain.

The mining process is multi-threaded, where several workers search for a special number called a `nonce`. The goal is to find a `nonce` that produces a valid block hash when combined with the other block data. A hash is considered valid if it meets the network's difficulty requirement, which in this case is a certain number of leading zeros. The hashing algorithm used is SHA-256.

When a worker finds a valid `nonce`, the new block is created and added to the chain.