#![no_std]

use soroban_sdk::address_payload::AddressPayload;
use soroban_sdk::{
    contract, contractimpl, contractmeta, token, Address, Bytes, BytesN, Env, String,
};
use stellar_access::ownable::{self as ownable};
use stellar_contract_utils::pausable::{self as pausable, Pausable};
use stellar_macros::{only_owner, when_not_paused};

mod errors;
mod events;
mod storage;

use crate::events::{
    BurnWithRedemption, Clawback, Initialized, MintInterest, MintWithReceipt, SacAdminSet,
    SacAuthorizationSet,
};
use crate::storage::DataKey;
pub use errors::Error;

contractmeta!(key = "name", val = "YLDS Controller");

contractmeta!(
    key = "description",
    val =
        "Figure YLDS Stellar controller: owner-gated mint/burn via SAC, replay keys, upgradeable.",
);

// SAC 7 dp vs uYLDS 6 dp; mint/burn `amount` must be a multiple of this.
const STELLAR_STROOPS_PER_UYLDS: i128 = 10;

fn require_stroops_align_with_uyls(amount: i128) -> Result<(), Error> {
    if amount % STELLAR_STROOPS_PER_UYLDS != 0 {
        return Err(Error::InvalidAmountPrecision);
    }
    Ok(())
}

#[contract]
pub struct YldsControllerContract;

#[contractimpl]
impl YldsControllerContract {
    fn build_ylds_asset(env: &Env, issuer: &Address) -> Result<Bytes, Error> {
        let payload = issuer.to_payload().ok_or(Error::InvalidIssuerAddress)?;

        let AddressPayload::AccountIdPublicKeyEd25519(public_key) = payload else {
            return Err(Error::InvalidIssuerAddress);
        };

        // Stellar Asset XDR for CreditAlphanum4(YLDS):
        // Asset discriminant (1) + AssetCode4("YLDS") + PublicKey discriminant (0) + issuer ed25519 bytes.
        let mut serialized =
            Bytes::from_slice(env, &[0, 0, 0, 1, b'Y', b'L', b'D', b'S', 0, 0, 0, 0]);
        serialized.append(public_key.as_bytes());

        Ok(serialized)
    }

    /// Soroban instance TTL: bump toward `max_ttl()` when remaining TTL falls below half
    /// of that cap (standard Soroban pattern).
    ///
    /// Operational: if there may be no invocations for longer than the network instance TTL
    /// window, run a periodic unauthenticated invoke of [`Self::get_sac`] or use
    /// `extend_footprint_ttl` / restore flows for this contract id.
    fn extend_instance_ttl(env: &Env) {
        let max_ttl = env.storage().max_ttl();
        let threshold = max_ttl / 2;
        env.storage().instance().extend_ttl(threshold, max_ttl);
    }

    pub fn version() -> u32 {
        4
    }

    pub fn __constructor(env: &Env, owner: Address) {
        // NOTE: Ensure the admin has authorized initialization
        owner.require_auth();
        ownable::set_owner(env, &owner);

        let serialized_asset = match Self::build_ylds_asset(env, &owner) {
            Ok(bytes) => bytes,
            Err(err) => soroban_sdk::panic_with_error!(&env, err),
        };

        // Resolve the predeployed YLDS SAC. Operational: the SAC has its own TTL; monitor
        // and extend or restore it via Stellar tooling when needed. This controller only
        // extends its own contract instance.
        let sac = env
            .deployer()
            .with_stellar_asset(serialized_asset)
            .deployed_address();

        let sac_client = token::StellarAssetClient::new(env, &sac);
        let sac_admin = sac_client.admin();

        // Check that the asset was built with the admin passed to constructor
        if sac_admin != owner {
            soroban_sdk::panic_with_error!(&env, Error::InvalidSacAdmin);
        }

        env.storage().instance().set(&DataKey::Sac, &sac);

        // Delegate admin of SAC from owner to controller
        let controller = env.current_contract_address();
        sac_client.set_admin(&controller);
        sac_client.set_authorized(&controller, &true);

        Self::extend_instance_ttl(env);
        Initialized { owner, sac }.publish(env);
    }

    /// Returns the deployed YLDS SAC address. Public so any caller can refresh instance
    /// TTL (e.g. cron/keepalive) without owner auth.
    pub fn get_sac(env: Env) -> Result<Address, Error> {
        Self::extend_instance_ttl(&env);
        storage::load_sac_address(&env)
    }

    pub fn get_owner(env: Env) -> Option<Address> {
        ownable::get_owner(&env)
    }

    pub fn transfer_ownership(env: Env, new_owner: Address, live_until_ledger: u32) {
        Self::extend_instance_ttl(&env);
        ownable::transfer_ownership(&env, &new_owner, live_until_ledger);
    }

    pub fn accept_ownership(env: Env) {
        Self::extend_instance_ttl(&env);
        ownable::accept_ownership(&env);
    }

    #[only_owner]
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        Self::extend_instance_ttl(&env);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Transfers Stellar asset (SAC) admin from this contract to `new_admin` (e.g. migration
    /// or recovery). Callable only by the contract owner.
    #[only_owner]
    pub fn set_sac_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        let sac = storage::load_sac_address(&env)?;
        let sac_client = token::StellarAssetClient::new(&env, &sac);
        sac_client.set_admin(&new_admin);
        SacAdminSet { new_admin }.publish(&env);

        Ok(())
    }

    #[only_owner]
    pub fn set_authorized(env: Env, address: Address, authorized: bool) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        if !authorized && address == env.current_contract_address() {
            return Err(Error::CannotDeauthorizeController);
        }

        let sac = storage::load_sac_address(&env)?;
        let sac_client = token::StellarAssetClient::new(&env, &sac);
        sac_client.set_authorized(&address, &authorized);
        SacAuthorizationSet {
            address,
            authorized,
        }
        .publish(&env);

        Ok(())
    }

    #[only_owner]
    #[when_not_paused]
    pub fn mint_with_receipt(
        env: Env,
        recipient: Address,
        amount: i128,
        asset: String,
        chain: String,
        tx_id: String,
    ) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        require_stroops_align_with_uyls(amount)?;

        let receipt_key = storage::receipt_replay_key(&env, &asset, &chain, &tx_id);

        if env.storage().persistent().has(&receipt_key) {
            return Err(Error::DuplicateReceipt);
        }
        storage::set_and_extend_replay_key(&env, &receipt_key);

        let sac = storage::load_sac_address(&env)?;
        let sac_client = token::StellarAssetClient::new(&env, &sac);
        sac_client.mint(&recipient, &amount);
        MintWithReceipt {
            asset,
            chain,
            tx_id,
            recipient,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    #[only_owner]
    #[when_not_paused]
    pub fn mint_interest(
        env: Env,
        recipient: Address,
        amount: i128,
        distribution_id: String,
    ) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        require_stroops_align_with_uyls(amount)?;

        let credit_key = storage::interest_replay_key(&env, &distribution_id);
        if env.storage().persistent().has(&credit_key) {
            return Err(Error::DuplicateInterestCredit);
        }
        storage::set_and_extend_replay_key(&env, &credit_key);

        let sac = storage::load_sac_address(&env)?;
        let sac_client = token::StellarAssetClient::new(&env, &sac);
        sac_client.mint(&recipient, &amount);
        MintInterest {
            distribution_id,
            recipient,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    #[only_owner]
    #[when_not_paused]
    pub fn burn_with_redemption(
        env: Env,
        amount: i128,
        asset: String,
        chain: String,
        redemption_id: String,
    ) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        require_stroops_align_with_uyls(amount)?;

        let redemption_key = storage::redemption_replay_key(&env, &asset, &chain, &redemption_id);
        if env.storage().persistent().has(&redemption_key) {
            return Err(Error::DuplicateRedemption);
        }
        storage::set_and_extend_replay_key(&env, &redemption_key);

        let sac = storage::load_sac_address(&env)?;

        // NOTE: We assume here that the contract address receives all tokens for burning
        let controller = env.current_contract_address();
        let token_client = token::TokenClient::new(&env, &sac);
        token_client.burn(&controller, &amount);

        BurnWithRedemption {
            redemption_id,
            controller,
            asset,
            chain,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    #[only_owner]
    #[when_not_paused]
    pub fn clawback(
        env: Env,
        target: Address,
        amount: i128,
        clawback_id: String,
    ) -> Result<(), Error> {
        Self::extend_instance_ttl(&env);

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if clawback_id.is_empty() {
            return Err(Error::InvalidClawbackId);
        }

        let clawback_key = storage::clawback_replay_key(&env, &clawback_id);
        if env.storage().persistent().has(&clawback_key) {
            return Err(Error::DuplicateClawback);
        }
        storage::set_and_extend_replay_key(&env, &clawback_key);

        let sac = storage::load_sac_address(&env)?;

        let token_client = token::StellarAssetClient::new(&env, &sac);

        token_client.clawback(&target, &amount);

        Clawback {
            clawback_id,
            target,
            amount,
        }
        .publish(&env);

        Ok(())
    }
}

#[contractimpl]
impl Pausable for YldsControllerContract {
    fn paused(env: &Env) -> bool {
        pausable::paused(env)
    }

    #[only_owner]
    fn pause(env: &Env, _caller: Address) {
        pausable::pause(env);
    }

    #[only_owner]
    fn unpause(env: &Env, _caller: Address) {
        pausable::unpause(env);
    }
}

mod test;
