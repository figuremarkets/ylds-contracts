use soroban_sdk::{contracttype, xdr::ToXdr, Address, BytesN, Env, String};

use crate::errors::Error;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Sac,
    Receipt(BytesN<32>),
    Redemption(BytesN<32>),
    InterestCredit(BytesN<32>),
    Clawback(BytesN<32>),
}

const RECEIPT_REPLAY_DOMAIN: u32 = 1;
const REDEMPTION_REPLAY_DOMAIN: u32 = 2;
const INTEREST_REPLAY_DOMAIN: u32 = 3;
const CLAWBACK_REPLAY_DOMAIN: u32 = 4;

// Replay history is stored off-chain; this only covers short retry windows.
const REPLAY_KEY_TTL_LEDGERS: u32 = 17_280;

fn replay_hash<T: ToXdr>(env: &Env, preimage: T) -> BytesN<32> {
    env.crypto().sha256(&preimage.to_xdr(env)).to_bytes()
}

pub fn receipt_replay_key(env: &Env, asset: &String, chain: &String, tx_id: &String) -> DataKey {
    DataKey::Receipt(replay_hash(
        env,
        (RECEIPT_REPLAY_DOMAIN, asset, chain, tx_id),
    ))
}

pub fn interest_replay_key(env: &Env, distribution_id: &String) -> DataKey {
    DataKey::InterestCredit(replay_hash(env, (INTEREST_REPLAY_DOMAIN, distribution_id)))
}

pub fn redemption_replay_key(
    env: &Env,
    asset: &String,
    chain: &String,
    redemption_id: &String,
) -> DataKey {
    DataKey::Redemption(replay_hash(
        env,
        (REDEMPTION_REPLAY_DOMAIN, asset, chain, redemption_id),
    ))
}

pub fn clawback_replay_key(env: &Env, clawback_id: &String) -> DataKey {
    DataKey::Clawback(replay_hash(env, (CLAWBACK_REPLAY_DOMAIN, clawback_id)))
}

pub fn load_sac_address(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Sac)
        .ok_or(Error::NotInitialized)
}

pub fn set_and_extend_replay_key(env: &Env, key: &DataKey) {
    env.storage().persistent().set(key, &true);
    let threshold = REPLAY_KEY_TTL_LEDGERS / 2;
    env.storage()
        .persistent()
        .extend_ttl(key, threshold, REPLAY_KEY_TTL_LEDGERS);
}
