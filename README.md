# Distributed Hash Table (Kademlia-style)

This crate contains a minimal Kademlia-style DHT implementation in Rust.

Features
- NodeId (256-bit) with XOR distance
- Routing table with k-buckets (K=20)
- Basic Kademlia RPC message types (Ping, FindNode, FindValue, Store)
- UDP transport using `tokio` for sending/receiving messages
- Iterative FIND_NODE implementation (simple variant)

Quick start

1. Start a node that listens on a port:

```bash
cargo run -- serve 14000
```

2. Start another node in a different terminal:

```bash
cargo run -- serve 14001
```

3. From a third terminal you can send a Ping using the test utilities or call into the running nodes programmatically. The demo `serve` command shows how to bind and serve; use the library to implement more advanced CLI.

Running integration tests (flaky / opt-in)

Integration tests that start multiple in-process network listeners are marked `#[ignore]` because they can be flaky on some platforms (macOS) when run under the test harness. To run them explicitly:

```bash
# run only ignored tests directly
cargo test -- --ignored --nocapture

# or use the convenience cargo alias added to Cargo.toml
cargo integration

# or run the helper script
./scripts/run-integration-tests.sh
```

Design notes
- This is a minimal, educational implementation: it does not implement persistence, routing table bucket splitting, replacement strategies in full, or cryptographic node identity.
- Uses `bincode` for compact binary serialization of RPC messages.
- The `find_node` implementation is a simple iterative variant with concurrency alpha=3.

Next steps
- Implement STORE/FindValue with local storage and replication
- Advanced bucket management (split, eviction with replacement caches)
- NAT traversal and reliable transports (TCP/quic)
- Integration with libp2p or other networking stacks

License: MIT
