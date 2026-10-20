# ylds-contracts

Open-source contracts for YLDS. Each chain lives in its own directory.

## Layout

| Directory | What it contains |
| --- | --- |
| [`Stellar/`](Stellar/) | Soroban controller for YLDS mint and burn on Stellar |

The Stellar contract, entrypoints, and operator scripts are documented in [`Stellar/README.md`](Stellar/README.md).

## Build

From `Stellar/`:

```bash
cargo test
cargo clippy --all-targets
stellar contract build --manifest-path contracts/ylds-controller/Cargo.toml
```

## License

Apache License 2.0. See [LICENSE](LICENSE).
