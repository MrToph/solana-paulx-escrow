# solana-escrow
Extends [paulx' Solana escrow tutorial](https://paulx.dev/blog/2021/01/14/programming-on-solana-an-introduction/) with [integration tests](./tests/integration.rs) to check correctness of the code.

### Environment Setup
1. Install Rust from https://rustup.rs/
2. Install Solana v1.7 or later from https://docs.solana.com/cli/install-solana-cli-tools#use-solanas-install-tool

### Build and test the program compiled for BPF

```bash
$ cargo build-bpf
# this runs the integration test
$ cargo test-bpf
```
