use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    DuplicateReceipt = 3,
    DuplicateRedemption = 4,
    InvalidSacAdmin = 5,
    InvalidIssuerAddress = 6,
    DuplicateInterestCredit = 7,
    InvalidAmountPrecision = 8,
    DuplicateClawback = 9,
    InvalidClawbackId = 10,
    CannotDeauthorizeController = 11,
}
