use sov_modules_api::{CryptoSpec, DaSpec, PrivateKey, Spec};

/// A representation of a simple user that is not staked at genesis.
#[derive(Debug, Clone)]
pub struct User<S: Spec> {
    private_key: <S::CryptoSpec as CryptoSpec>::PrivateKey,
    balance: u64,
}

impl<S: Spec> User<S> {
    /// Creates a new user with the given private key and balance.
    pub fn new(private_key: <S::CryptoSpec as CryptoSpec>::PrivateKey, balance: u64) -> Self {
        Self {
            private_key,
            balance,
        }
    }

    pub fn generate(balance: u64) -> Self {
        Self {
            private_key: <<S as Spec>::CryptoSpec as CryptoSpec>::PrivateKey::generate(),
            balance,
        }
    }

    /// Returns the address of the user.
    pub fn address(&self) -> <S as Spec>::Address {
        <S as Spec>::Address::from(&self.private_key.pub_key())
    }

    /// Returns the private key of the user.
    pub fn private_key(&self) -> &<S::CryptoSpec as CryptoSpec>::PrivateKey {
        &self.private_key
    }

    /// Returns the balance of the user.
    pub fn balance(&self) -> u64 {
        self.balance
    }
}

pub trait StakedUser<S: Spec>: Into<User<S>> {
    /// Returns the private key of the staked user.
    fn private_key(&self) -> &<S::CryptoSpec as CryptoSpec>::PrivateKey;

    /// Only returns the bank balance of the staked user. Ie, the balance that is not staked.
    fn free_balance(&self) -> u64;

    /// Returns the bond amount of the staked user.
    fn bond(&self) -> u64;

    /// The total balance of the staked user, including the bond and any additional balance.
    fn total_balance(&self) -> u64 {
        self.bond() + self.free_balance()
    }

    /// Compute and return the address of the staked user.
    fn address(&self) -> S::Address {
        <S as Spec>::Address::from(&self.private_key().pub_key())
    }
}

impl<S: Spec> From<SimpleStakedUser<S>> for User<S> {
    fn from(staked_user: SimpleStakedUser<S>) -> Self {
        Self {
            private_key: staked_user.private_key,
            balance: staked_user.additional_balance.unwrap_or_default(),
        }
    }
}

impl<S: Spec, Da: DaSpec> From<Sequencer<S, Da>> for User<S> {
    fn from(sequencer: Sequencer<S, Da>) -> Self {
        Self {
            private_key: sequencer.private_key,
            balance: sequencer.additional_balance.unwrap_or_default(),
        }
    }
}

/// A simple representation of a user that is staked at genesis.
#[derive(Debug, Clone)]
pub struct SimpleStakedUser<S: Spec> {
    pub private_key: <S::CryptoSpec as CryptoSpec>::PrivateKey,
    /// The amount of tokens to bond at genesis. These tokens will be minted by the bank.
    pub bond: u64,
    /// Any additional (not bonded) balance that the bank should mint for the user.
    pub additional_balance: Option<u64>,
}

impl<S: Spec> StakedUser<S> for SimpleStakedUser<S> {
    /// Returns the private key of the staked user.
    fn private_key(&self) -> &<S::CryptoSpec as CryptoSpec>::PrivateKey {
        &self.private_key
    }

    /// Only returns the bank balance of the staked user. Ie, the balance that is not staked.
    fn free_balance(&self) -> u64 {
        self.additional_balance.unwrap_or(0)
    }

    /// Returns the bond amount of the staked user.
    fn bond(&self) -> u64 {
        self.bond
    }
}

/// A representation of a sequencer at genesis.
#[derive(Debug, Clone)]
pub struct Sequencer<S: Spec, Da: DaSpec> {
    pub private_key: <S::CryptoSpec as CryptoSpec>::PrivateKey,
    pub da_address: Da::Address,
    /// The amount of tokens to bond at genesis. These tokens will be minted by the bank.
    pub bond: u64,
    /// Any additional (not bonded) balance that the bank should mint for the attester.
    pub additional_balance: Option<u64>,
}

impl<S: Spec, Da: DaSpec> StakedUser<S> for Sequencer<S, Da> {
    /// Returns the private key of the sequencer.
    fn private_key(&self) -> &<S::CryptoSpec as CryptoSpec>::PrivateKey {
        &self.private_key
    }

    /// Returns the stake amount of the sequencer.
    fn bond(&self) -> u64 {
        self.bond
    }

    /// Returns the balance of the sequencer.
    fn free_balance(&self) -> u64 {
        self.additional_balance.unwrap_or(0)
    }
}

impl<S: Spec, Da: DaSpec> Sequencer<S, Da> {
    /// Returns the DA address of the sequencer.
    pub fn da_address(&self) -> &Da::Address {
        &self.da_address
    }
}
