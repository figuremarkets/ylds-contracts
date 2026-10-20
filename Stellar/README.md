# ylds-controller (Stellar)

Soroban controller contract for YLDS mint/burn flows on Stellar.

## Scope

- Owns replay protection for mint and burn requests.
- Calls SAC for token operations (`mint`, `burn`, `set_admin`).
- Keeps operator-controlled admin handoff/recovery path.

## Contract layout

- `contracts/ylds-controller/src/lib.rs` - contract entrypoints.
- `contracts/ylds-controller/src/errors.rs` - contract error codes.
- `contracts/ylds-controller/src/events.rs` - emitted contract events.
- `contracts/ylds-controller/src/storage.rs` - storage keys and helpers.

## Implemented entrypoints

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

## Pause semantics

- `pause` and `unpause` are owner-gated OpenZeppelin Pausable controls.
- While paused, token mutators are blocked: deposit mint, interest mint, burn, and clawback.
- Admin and recovery entrypoints remain callable while paused: `get_sac`, `get_owner`, `transfer_ownership`, `accept_ownership`, `upgrade`, `set_sac_admin`, and `set_authorized`.
- Paused attempts do not write replay keys, so the same operation can be retried after unpause.

## Replay semantics

- Deposit mint: `(asset, chain, tx_id)`
- Interest mint: `distribution_id`
- Burn: `(asset, chain, redemption_id)`
- Duplicate calls return contract errors (`DuplicateReceipt`, `DuplicateInterestCredit`, `DuplicateRedemption`, `DuplicateClawback`).
- Replay uniqueness on-chain is TTL-bounded.
- Permanent dedupe authority is the service operation store (off-chain).

## Amount semantics

- Invoke expects SAC stroops. Mint and burn amounts must be multiples of `STELLAR_STROOPS_PER_UYLDS` (10); clawback accepts any positive SAC stroop amount.

## Input limits

- `asset`: max 32 chars
- `chain`: max 32 chars
- `tx_id`: max 128 chars
- `redemption_id`: max 128 chars
- `distribution_id`: max 128 chars

## Build and test

From `Stellar/`:

```bash
cargo test
cargo clippy --all-targets
stellar contract build --manifest-path contracts/ylds-controller/Cargo.toml
```

## Operator scripts

- Deploy: `scripts/chains/stellar/deploy-controller.sh`
- Upgrade: `scripts/chains/stellar/upgrade-contract.sh`

## Notes

- Storage on Soroban is TTL-based; replay keys are extended to max TTL on write.
- This contract assumes operator is the only privileged caller for state-changing methods.
- Constructor resolves the predeployed SAC from the serialized Stellar asset and atomically sets SAC admin to controller.
- The contract is upgradeable: owner can swap contract Wasm at the same contract address using `upgrade(new_wasm_hash)`.
- Upgrades preserve existing storage (`Sac`, replay keys, owner) and do not re-run `__constructor`.
- Event topics use the Soroban topic budget and index cross-chain route keys for filtering:
  - `MintWithReceipt`: `asset`, `chain`, `tx_id`
  - `MintInterest`: `distribution_id`
  - `BurnWithRedemption`: `asset`, `chain`, `redemption_id`
  - `Clawback`: `clawback_id`, `target`
  - `SacAuthorizationSet`: `address`

## What is native vs custom

- Native (Stellar/Soroban):
  - Token admin/mint/burn flows via SAC (`StellarAssetClient`, `TokenClient`).
  - Contract event indexing/topic model.
  - TTL archival model and extension APIs.
- Custom (required for YLDS parity):
  - Replay registry keyed by `tx_id` and `redemption_id`.
  - Duplicate rejection semantics.
  - Burn execution uses operator-controlled signer context (no caller-provided burn source).
  - Burn event includes operator (not caller-provided source).
