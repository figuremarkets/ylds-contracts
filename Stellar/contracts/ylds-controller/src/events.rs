use soroban_sdk::{contractevent, Address, String};

#[contractevent]
pub struct Initialized {
    #[topic]
    pub owner: Address,
    #[topic]
    pub sac: Address,
}

#[contractevent]
pub struct SacAdminSet {
    #[topic]
    pub new_admin: Address,
}

#[contractevent]
pub struct SacAuthorizationSet {
    #[topic]
    pub address: Address,
    pub authorized: bool,
}

#[contractevent]
pub struct MintWithReceipt {
    #[topic]
    pub asset: String,
    #[topic]
    pub chain: String,
    #[topic]
    pub tx_id: String,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent]
pub struct MintInterest {
    #[topic]
    pub distribution_id: String,
    pub recipient: Address,
    pub amount: i128,
}

#[contractevent]
pub struct BurnWithRedemption {
    #[topic]
    pub asset: String,
    #[topic]
    pub chain: String,
    #[topic]
    pub redemption_id: String,
    pub controller: Address,
    pub amount: i128,
}

#[contractevent]
pub struct Clawback {
    #[topic]
    pub clawback_id: String,
    #[topic]
    pub target: Address,
    pub amount: i128,
}
