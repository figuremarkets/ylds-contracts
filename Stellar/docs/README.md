# Stellar

Stellar includes an on-chain controller contract used for YLDS mint and burn flows.

## Contract Location

- Workspace: `Stellar/`
- Contract crate: `contracts/ylds-controller`
- Contract README: `README.md`

## Implemented Entrypoints

- `__constructor(owner)`
- `version()`
- `get_sac()`
- `get_owner()`
- `transfer_ownership(new_owner, live_until_ledger)`
- `accept_ownership()`
- `paused()`
- `pause(caller)`
- `unpause(caller)`
- `upgrade(new_wasm_hash)`
- `set_sac_admin(new_admin)`
- `set_authorized(address, authorized)`
- `mint_with_receipt(recipient, amount, asset, chain, tx_id)`
- `mint_interest(recipient, amount, distribution_id)`
- `burn_with_redemption(amount, asset, chain, redemption_id)`
- `clawback(target, amount, clawback_id)`

## Behavior

- Constructor resolves the predeployed YLDS SAC and transfers SAC admin to the controller contract.
- State-changing entrypoints require owner auth.
- While paused, deposit mint, interest mint, burn, and clawback are blocked. `get_sac`, `get_owner`, `transfer_ownership`, `accept_ownership`, `upgrade`, `set_sac_admin`, and `set_authorized` stay callable.
- Contract is upgradeable: owner can update Wasm while keeping the same contract address and storage.
- Mint/burn replay keys are persisted to reject duplicate operations.
- Replay keys and instance state extend TTL toward max TTL.
- Contract emits `Initialized`, `SacAdminSet`, `SacAuthorizationSet`, `MintWithReceipt`, `MintInterest`, `BurnWithRedemption`, and `Clawback`.

## Build and Test

From `Stellar/`:

```bash
cargo test
cargo clippy --all-targets
cargo build --target wasm32v1-none --release
```

## Deployment Utilities

- Create issuer + flags: `scripts/chains/stellar/create-issuer.sh`
- Deploy controller (script predeploys SAC): `scripts/chains/stellar/deploy-controller.sh`
- Stellar upgrade utility: `scripts/chains/stellar/upgrade-contract.sh`

Scripts below target `testnet` / `mainnet`.

Example flow:

```bash
./scripts/chains/stellar/create-issuer.sh --network testnet --issuer ylds-issuer
```

Mainnet note: before running `deploy-controller.sh` on `mainnet`, ensure the issuer/owner account is funded with XLM (for account reserve and transaction fees).

```bash
./scripts/chains/stellar/deploy-controller.sh --network testnet --owner ylds-issuer --alias ylds-controller
```

## Key Commands

- Get issuer public key: `stellar keys public-key <alias>`

Example:

```bash
stellar keys public-key ylds-issuer
```

Keep issuer secrets out of logs, docs, and chat.

The upgrade script is intended for repeatable operator upgrades and supports the same networks. It builds (unless `--skip-build`), uploads new wasm, invokes `upgrade(new_wasm_hash)` as owner, validates with `version`, and prints `STELLAR_*` upgrade outputs.

## Notes

- Current replay uniqueness is bounded by Soroban storage TTL.
- Off-chain operation tracking remains the long-term dedupe authority.
