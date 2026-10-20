#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::testutils::{
    AuthorizedFunction, AuthorizedInvocation, IssuerFlags, MockAuth, MockAuthInvoke,
};
use soroban_sdk::{
    symbol_short, testutils::Address as _, xdr::ToXdr, Address, BytesN, Env, IntoVal, String,
};

fn generate_account_address(env: &Env) -> Address {
    let temp_admin = Address::generate(env);
    let temp_sac = env.register_stellar_asset_contract_v2(temp_admin);
    temp_sac.issuer().set_flag(IssuerFlags::ClawbackEnabledFlag);
    temp_sac.issuer().set_flag(IssuerFlags::RevocableFlag);
    temp_sac.issuer().address()
}

fn mint_to_contract_balance(
    client: &YldsControllerContractClient<'_>,
    env: &Env,
    contract_id: &Address,
    tx_id: &str,
) {
    let result = client.try_mint_with_receipt(
        contract_id,
        &10,
        &String::from_str(env, "USDC"),
        &String::from_str(env, "ETH"),
        &String::from_str(env, tx_id),
    );
    assert_eq!(result, Ok(Ok(())));
}

fn deploy_with_auth() -> (
    Env,
    Address,
    YldsControllerContractClient<'static>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let issuer = generate_account_address(&env);
    let serialized_asset = YldsControllerContract::build_ylds_asset(&env, &issuer).unwrap();
    let expected_sac = env.deployer().with_stellar_asset(serialized_asset).deploy();

    let contract_id = env.register(YldsControllerContract, (issuer.clone(),));
    let client = YldsControllerContractClient::new(&env, &contract_id);
    let sac = client.get_sac();
    assert_eq!(sac, expected_sac);

    (env, contract_id, client, issuer, sac)
}

fn deploy_without_runtime_auth() -> (Env, YldsControllerContractClient<'static>) {
    let (env, _contract_id, client, _issuer, _sac) = deploy_with_auth();
    env.set_auths(&[]);
    (env, client)
}

fn long_string(env: &Env, byte: u8, len: usize) -> String {
    let bytes = std::vec![byte; len];
    String::from_bytes(env, &bytes)
}

fn replay_key_xdr_len(env: &Env, key: storage::DataKey) -> u32 {
    key.to_xdr(env).len()
}

#[test]
fn constructor_sets_state_and_transfers_sac_admin() {
    let (env, contract_id, client, _, sac) = deploy_with_auth();

    assert_eq!(client.get_sac(), sac);
    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    assert_eq!(sac_client.admin(), contract_id);
}

#[test]
fn get_owner_returns_current_owner() {
    let (_env, _contract_id, client, issuer, _sac) = deploy_with_auth();

    assert_eq!(client.get_owner(), Some(issuer));
}

#[test]
fn owner_can_initiate_transfer_and_pending_owner_can_accept() {
    let (env, _contract_id, client, issuer, _sac) = deploy_with_auth();
    let new_owner = Address::generate(&env);

    client.transfer_ownership(&new_owner, &1000);
    assert_eq!(client.get_owner(), Some(issuer));

    client.accept_ownership();
    assert_eq!(client.get_owner(), Some(new_owner));
}

#[test]
fn non_owner_cannot_initiate_transfer() {
    let (env, contract_id, client, _issuer, _sac) = deploy_with_auth();
    let non_owner = Address::generate(&env);
    let new_owner = Address::generate(&env);
    let live_until_ledger = 1000;

    env.set_auths(&[]);

    let result = client
        .mock_auths(&[MockAuth {
            address: &non_owner,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "transfer_ownership",
                args: (&new_owner, live_until_ledger).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_transfer_ownership(&new_owner, &live_until_ledger);

    assert!(result.is_err());
    assert_ne!(client.get_owner(), Some(new_owner));
}

#[test]
fn ownership_transfer_updates_owner_gated_access() {
    let (env, contract_id, client, issuer, _sac) = deploy_with_auth();
    let new_owner = Address::generate(&env);
    let live_until_ledger = 1000;

    client.transfer_ownership(&new_owner, &live_until_ledger);
    client.accept_ownership();

    env.set_auths(&[]);

    let old_owner_args = (&issuer,).into_val(&env);
    let old_owner_auths = std::vec![MockAuth {
        address: &issuer,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pause",
            args: old_owner_args,
            sub_invokes: &[],
        },
    }
    .into()];
    env.set_auths(&old_owner_auths);
    let old_owner_result = client.try_pause(&issuer);
    assert!(old_owner_result.is_err());

    assert_eq!(client.get_owner(), Some(new_owner.clone()));
    env.mock_all_auths();
    assert_eq!(client.try_pause(&new_owner), Ok(Ok(())));
    assert_eq!(
        env.auths(),
        std::vec![(
            new_owner.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    contract_id,
                    symbol_short!("pause"),
                    (&new_owner,).into_val(&env),
                )),
                sub_invocations: std::vec![],
            },
        )]
    );
}

#[test]
fn pause_toggles_state_and_unpause_restores_operations() {
    let (env, _contract_id, client, issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let tx_id = String::from_str(&env, "tx-paused-then-unpaused");

    assert!(!client.paused());
    client.pause(&issuer);
    assert!(client.paused());

    assert!(client
        .try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &tx_id,
        )
        .is_err());

    client.unpause(&issuer);
    assert!(!client.paused());

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &tx_id,
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 10);
}

#[test]
fn pause_blocks_all_token_mutators_without_locking_replay_keys() {
    let (env, contract_id, client, issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let target = Address::generate(&env);
    let mint_tx_id = String::from_str(&env, "tx-paused-mint");
    let interest_id = String::from_str(&env, "dist-paused");
    let redemption_id = String::from_str(&env, "red-paused");
    let clawback_id = String::from_str(&env, "claw-paused");

    mint_to_contract_balance(&client, &env, &contract_id, "tx-paused-burn-seed");
    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-paused-claw-seed"),
        ),
        Ok(Ok(()))
    );

    client.pause(&issuer);

    assert!(client
        .try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &mint_tx_id,
        )
        .is_err());
    assert!(client
        .try_mint_interest(&recipient, &10, &interest_id)
        .is_err());
    assert!(client
        .try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &redemption_id,
        )
        .is_err());
    assert!(client.try_clawback(&target, &10, &clawback_id).is_err());

    client.unpause(&issuer);

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &mint_tx_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_mint_interest(&recipient, &10, &interest_id),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &redemption_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(client.try_clawback(&target, &10, &clawback_id), Ok(Ok(())));

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&target), 0);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn pause_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();

    client.pause(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn unpause_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();

    client.unpause(&Address::generate(&env));
}

#[test]
fn mint_with_receipt_rejects_invalid_inputs() {
    let (env, _contract_id, client, _issuer, _sac) = deploy_with_auth();
    let recipient = Address::generate(&env);

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &0,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-1"),
        ),
        Err(Ok(Error::InvalidAmount))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &1,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-precision"),
        ),
        Err(Ok(Error::InvalidAmountPrecision))
    );
}

#[test]
fn mint_interest_mints_and_blocks_duplicate_distribution_ids() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let dist_id = String::from_str(&env, "dist-1");

    assert_eq!(
        client.try_mint_interest(&recipient, &10, &dist_id),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_interest(&recipient, &10, &dist_id),
        Err(Ok(Error::DuplicateInterestCredit))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 10);
}

#[test]
fn mint_interest_rejects_invalid_precision() {
    let (env, _contract_id, client, _issuer, _sac) = deploy_with_auth();
    let recipient = Address::generate(&env);

    assert_eq!(
        client.try_mint_interest(&recipient, &1, &String::from_str(&env, "dist-precision"),),
        Err(Ok(Error::InvalidAmountPrecision))
    );
}

#[test]
fn mint_interest_and_deposit_use_distinct_replay_keys() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let shared_id = String::from_str(&env, "shared-key");

    assert_eq!(
        client.try_mint_interest(&recipient, &10, &shared_id),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &shared_id,
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 20);
}

#[test]
fn replay_key_size_does_not_grow_with_input_length() {
    let env = Env::default();
    let asset = String::from_str(&env, "USDC");
    let chain = String::from_str(&env, "ETH");
    let short_id = String::from_str(&env, "id");
    let long_id = long_string(&env, b'x', 4096);

    assert_eq!(
        replay_key_xdr_len(
            &env,
            storage::receipt_replay_key(&env, &asset, &chain, &short_id),
        ),
        replay_key_xdr_len(
            &env,
            storage::receipt_replay_key(&env, &asset, &chain, &long_id)
        )
    );
    assert_eq!(
        replay_key_xdr_len(&env, storage::interest_replay_key(&env, &short_id)),
        replay_key_xdr_len(&env, storage::interest_replay_key(&env, &long_id))
    );
    assert_eq!(
        replay_key_xdr_len(
            &env,
            storage::redemption_replay_key(&env, &asset, &chain, &short_id),
        ),
        replay_key_xdr_len(
            &env,
            storage::redemption_replay_key(&env, &asset, &chain, &long_id),
        )
    );
    assert_eq!(
        replay_key_xdr_len(&env, storage::clawback_replay_key(&env, &short_id)),
        replay_key_xdr_len(&env, storage::clawback_replay_key(&env, &long_id))
    );
}

#[test]
fn replay_namespaces_are_domain_separated() {
    let env = Env::default();
    let asset = String::from_str(&env, "USDC");
    let chain = String::from_str(&env, "ETH");
    let shared_id = String::from_str(&env, "shared-id");

    let receipt = storage::receipt_replay_key(&env, &asset, &chain, &shared_id).to_xdr(&env);
    let interest = storage::interest_replay_key(&env, &shared_id).to_xdr(&env);
    let redemption = storage::redemption_replay_key(&env, &asset, &chain, &shared_id).to_xdr(&env);
    let clawback = storage::clawback_replay_key(&env, &shared_id).to_xdr(&env);

    assert_ne!(receipt, interest);
    assert_ne!(receipt, redemption);
    assert_ne!(receipt, clawback);
    assert_ne!(interest, redemption);
    assert_ne!(interest, clawback);
    assert_ne!(redemption, clawback);
}

#[test]
fn different_replay_field_tuples_hash_to_different_keys() {
    let env = Env::default();
    let usdc = String::from_str(&env, "USDC");
    let eth = String::from_str(&env, "ETH");
    let polygon = String::from_str(&env, "POLYGON");
    let tx_1 = String::from_str(&env, "tx-1");
    let tx_2 = String::from_str(&env, "tx-2");

    assert_ne!(
        storage::receipt_replay_key(&env, &usdc, &eth, &tx_1).to_xdr(&env),
        storage::receipt_replay_key(&env, &usdc, &eth, &tx_2).to_xdr(&env)
    );
    assert_ne!(
        storage::receipt_replay_key(&env, &usdc, &eth, &tx_1).to_xdr(&env),
        storage::receipt_replay_key(&env, &usdc, &polygon, &tx_1).to_xdr(&env)
    );
    assert_ne!(
        storage::redemption_replay_key(&env, &usdc, &eth, &tx_1).to_xdr(&env),
        storage::redemption_replay_key(&env, &usdc, &polygon, &tx_1).to_xdr(&env)
    );
    assert_ne!(
        storage::interest_replay_key(&env, &tx_1).to_xdr(&env),
        storage::interest_replay_key(&env, &tx_2).to_xdr(&env)
    );
    assert_ne!(
        storage::clawback_replay_key(&env, &tx_1).to_xdr(&env),
        storage::clawback_replay_key(&env, &tx_2).to_xdr(&env)
    );
}

#[test]
fn same_id_can_be_used_across_all_replay_namespaces() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let target = Address::generate(&env);
    let shared_id = String::from_str(&env, "shared-all-namespaces");

    mint_to_contract_balance(&client, &env, &contract_id, "tx-shared-redemption-seed");
    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-shared-clawback-seed"),
        ),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &shared_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_mint_interest(&recipient, &10, &shared_id),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &shared_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(client.try_clawback(&target, &10, &shared_id), Ok(Ok(())));

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 20);
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(token_client.balance(&target), 0);
}

#[test]
fn long_replay_ids_follow_existing_input_behavior() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let target = Address::generate(&env);
    let long_tx_id = long_string(&env, b't', 512);
    let long_distribution_id = long_string(&env, b'd', 512);
    let long_redemption_id = long_string(&env, b'r', 512);
    let long_clawback_id = long_string(&env, b'c', 512);

    mint_to_contract_balance(&client, &env, &contract_id, "tx-long-redemption-seed");
    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-long-clawback-seed"),
        ),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &long_tx_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_mint_interest(&recipient, &10, &long_distribution_id),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &long_redemption_id,
        ),
        Ok(Ok(()))
    );
    assert_eq!(
        client.try_clawback(&target, &10, &long_clawback_id),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 20);
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(token_client.balance(&target), 0);
}

#[test]
fn mint_with_receipt_mints_and_blocks_duplicate_receipts() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-dup"),
        ),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-dup"),
        ),
        Err(Ok(Error::DuplicateReceipt))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 10);
}

#[test]
fn mint_with_receipt_allows_same_tx_id_across_routes() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let recipient = Address::generate(&env);
    let tx_id = String::from_str(&env, "tx-shared");

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &tx_id,
        ),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_mint_with_receipt(
            &recipient,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "POLYGON"),
            &tx_id,
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&recipient), 20);
}

#[test]
fn mint_failed_attempt_does_not_lock_receipt_id() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();

    let new_admin = Address::generate(&env);
    assert_eq!(client.try_set_sac_admin(&new_admin), Ok(Ok(())));

    let recipient = Address::generate(&env);
    let tx_id = String::from_str(&env, "tx-retry");

    let first = client.try_mint_with_receipt(
        &recipient,
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &tx_id,
    );
    assert!(first.is_err());
    assert_ne!(first, Err(Ok(Error::DuplicateReceipt)));

    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    sac_client.set_admin(&contract_id);

    let second = client.try_mint_with_receipt(
        &recipient,
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &tx_id,
    );
    assert_eq!(second, Ok(Ok(())));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn mint_with_receipt_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();
    let recipient = Address::generate(&env);

    client.mint_with_receipt(
        &recipient,
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &String::from_str(&env, "tx-auth"),
    );
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn mint_interest_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();
    let recipient = Address::generate(&env);

    client.mint_interest(&recipient, &10, &String::from_str(&env, "dist-auth"));
}

#[test]
fn burn_with_redemption_rejects_invalid_inputs() {
    let (env, _contract_id, client, _issuer, _sac) = deploy_with_auth();

    assert_eq!(
        client.try_burn_with_redemption(
            &0,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "red-1"),
        ),
        Err(Ok(Error::InvalidAmount))
    );

    assert_eq!(
        client.try_burn_with_redemption(
            &1,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "red-precision"),
        ),
        Err(Ok(Error::InvalidAmountPrecision))
    );
}

#[test]
fn clawback_burns_from_target_balance() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let target = Address::generate(&env);
    let clawback_id = String::from_str(&env, "claw-1");

    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-claw-seed"),
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&target), 10);

    assert_eq!(client.try_clawback(&target, &10, &clawback_id), Ok(Ok(())));
    assert_eq!(token_client.balance(&target), 0);
}

#[test]
fn clawback_allows_non_uyls_aligned_positive_amount() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let target = Address::generate(&env);
    let clawback_id = String::from_str(&env, "claw-dust");

    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-claw-dust-seed"),
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&target), 10);

    assert_eq!(client.try_clawback(&target, &1, &clawback_id), Ok(Ok(())));
    assert_eq!(token_client.balance(&target), 9);
}

#[test]
fn clawback_rejects_non_positive_amounts() {
    let (env, _contract_id, client, _issuer, _sac) = deploy_with_auth();
    let target = Address::generate(&env);

    assert_eq!(
        client.try_clawback(&target, &0, &String::from_str(&env, "claw-zero")),
        Err(Ok(Error::InvalidAmount))
    );
    assert_eq!(
        client.try_clawback(&target, &-1, &String::from_str(&env, "claw-negative")),
        Err(Ok(Error::InvalidAmount))
    );
}

#[test]
fn clawback_blocks_duplicate_ids() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let target = Address::generate(&env);
    let clawback_id = String::from_str(&env, "claw-dup");

    assert_eq!(
        client.try_mint_with_receipt(
            &target,
            &20,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &String::from_str(&env, "tx-claw-dup-seed"),
        ),
        Ok(Ok(()))
    );

    assert_eq!(client.try_clawback(&target, &10, &clawback_id), Ok(Ok(())));
    assert_eq!(
        client.try_clawback(&target, &10, &clawback_id),
        Err(Ok(Error::DuplicateClawback))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&target), 10);
}

#[test]
fn burn_with_redemption_burns_and_blocks_duplicate_redemptions() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    mint_to_contract_balance(&client, &env, &contract_id, "tx-burn-seed");

    let redemption_id = String::from_str(&env, "red-dup");

    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &redemption_id,
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&contract_id), 0);

    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &redemption_id,
        ),
        Err(Ok(Error::DuplicateRedemption))
    );
}

#[test]
fn burn_with_redemption_allows_same_id_across_routes() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    mint_to_contract_balance(&client, &env, &contract_id, "tx-burn-route-1");
    mint_to_contract_balance(&client, &env, &contract_id, "tx-burn-route-2");

    let redemption_id = String::from_str(&env, "red-shared");

    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "ETH"),
            &redemption_id,
        ),
        Ok(Ok(()))
    );

    assert_eq!(
        client.try_burn_with_redemption(
            &10,
            &String::from_str(&env, "USDC"),
            &String::from_str(&env, "POLYGON"),
            &redemption_id,
        ),
        Ok(Ok(()))
    );

    let token_client = soroban_sdk::token::TokenClient::new(&env, &sac);
    assert_eq!(token_client.balance(&contract_id), 0);
}

#[test]
fn burn_failed_attempt_does_not_lock_redemption_id() {
    let (env, contract_id, client, _issuer, _sac) = deploy_with_auth();

    let redemption_id = String::from_str(&env, "red-retry");

    let first = client.try_burn_with_redemption(
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &redemption_id,
    );
    assert!(first.is_err());
    assert_ne!(first, Err(Ok(Error::DuplicateRedemption)));

    mint_to_contract_balance(&client, &env, &contract_id, "tx-burn-retry-seed");

    let second = client.try_burn_with_redemption(
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &redemption_id,
    );
    assert_eq!(second, Ok(Ok(())));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn burn_with_redemption_requires_issuer_auth() {
    let (env, contract_id, client, _issuer, _sac) = deploy_with_auth();
    mint_to_contract_balance(&client, &env, &contract_id, "tx-burn-auth-seed");
    env.set_auths(&[]);

    client.burn_with_redemption(
        &10,
        &String::from_str(&env, "USDC"),
        &String::from_str(&env, "ETH"),
        &String::from_str(&env, "red-auth"),
    );
}

#[test]
fn set_sac_admin_changes_sac_admin() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();

    let new_admin = Address::generate(&env);
    assert_eq!(client.try_set_sac_admin(&new_admin), Ok(Ok(())));

    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    assert_eq!(sac_client.admin(), new_admin);
}

#[test]
fn owner_can_authorize_address() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let address = Address::generate(&env);

    assert_eq!(client.try_set_authorized(&address, &true), Ok(Ok(())));

    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    assert!(sac_client.authorized(&address));
}

#[test]
fn owner_can_deauthorize_address() {
    let (env, _contract_id, client, _issuer, sac) = deploy_with_auth();
    let address = Address::generate(&env);
    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);

    assert_eq!(client.try_set_authorized(&address, &true), Ok(Ok(())));
    assert!(sac_client.authorized(&address));

    assert_eq!(client.try_set_authorized(&address, &false), Ok(Ok(())));
    assert!(!sac_client.authorized(&address));
}

#[test]
fn cannot_deauthorize_controller() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);

    assert!(sac_client.authorized(&contract_id));
    assert_eq!(
        client.try_set_authorized(&contract_id, &false),
        Err(Ok(Error::CannotDeauthorizeController))
    );
    assert!(sac_client.authorized(&contract_id));
}

#[test]
fn non_owner_cannot_set_authorized() {
    let (env, contract_id, client, _issuer, sac) = deploy_with_auth();
    let non_owner = Address::generate(&env);
    let address = Address::generate(&env);

    client.set_authorized(&address, &false);
    env.set_auths(&[]);

    let result = client
        .mock_auths(&[MockAuth {
            address: &non_owner,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_authorized",
                args: (&address, true).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_set_authorized(&address, &true);

    assert!(result.is_err());
    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    assert!(!sac_client.authorized(&address));
}

#[test]
fn set_authorized_works_while_paused() {
    let (env, _contract_id, client, issuer, sac) = deploy_with_auth();
    let address = Address::generate(&env);

    client.pause(&issuer);
    assert!(client.paused());

    assert_eq!(client.try_set_authorized(&address, &true), Ok(Ok(())));

    let sac_client = soroban_sdk::token::StellarAssetClient::new(&env, &sac);
    assert!(sac_client.authorized(&address));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn set_sac_admin_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();

    client.set_sac_admin(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn set_authorized_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();

    client.set_authorized(&Address::generate(&env), &true);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn upgrade_requires_issuer_auth() {
    let (env, client) = deploy_without_runtime_auth();
    let new_wasm_hash = BytesN::from_array(&env, &[0; 32]);

    client.upgrade(&new_wasm_hash);
}

#[test]
#[should_panic(expected = "Error(Auth, InvalidAction)")]
fn constructor_requires_auth() {
    let env = Env::default();

    let issuer = generate_account_address(&env);

    let _ = env.register(YldsControllerContract, (issuer,));
}
