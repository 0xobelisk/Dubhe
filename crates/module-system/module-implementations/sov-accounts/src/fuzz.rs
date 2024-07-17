use arbitrary::{Arbitrary, Unstructured};
use sov_modules_api::{CredentialId, CryptoSpec, Module, Spec, StateCheckpoint};

use crate::{Account, AccountConfig, AccountData, Accounts, CallMessage};

impl<'a> Arbitrary<'a> for CallMessage {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let credential_id = <[u8; 32]>::arbitrary(u)?;
        Ok(Self::InsertCredentialId(CredentialId(credential_id)))
    }
}

impl<'a, S> Arbitrary<'a> for Account<S>
where
    S: Spec,
    S::Address: Arbitrary<'a>,
{
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let addr = u.arbitrary()?;
        Ok(Self { addr })
    }
}

impl<'a, Addr: arbitrary::Arbitrary<'a>> Arbitrary<'a> for AccountData<Addr> {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            credential_id: CredentialId(u.arbitrary()?),
            address: u.arbitrary()?,
        })
    }
}

impl<'a, S> Arbitrary<'a> for AccountConfig<S>
where
    S: Spec,
    S::Address: Arbitrary<'a>,
    <S::CryptoSpec as CryptoSpec>::PublicKey: Arbitrary<'a>,
{
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            accounts: u.arbitrary_iter()?.collect::<Result<_, _>>()?,
        })
    }
}

impl<'a, S> Accounts<S>
where
    S: Spec,
    S::Address: Arbitrary<'a>,
    <S::CryptoSpec as CryptoSpec>::PublicKey: Arbitrary<'a>,
{
    /// Creates an arbitrary set of accounts and stores it under `state`.
    pub fn arbitrary_workset(
        u: &mut Unstructured<'a>,
        state: StateCheckpoint<S>,
    ) -> (arbitrary::Result<Self>, StateCheckpoint<S>) {
        let config: AccountConfig<S> = match u.arbitrary() {
            Ok(config) => config,
            Err(e) => return (Err(e), state),
        };
        let accounts = Accounts::default();
        let mut genesis_state = state.to_genesis_state_accessor::<Accounts<S>>(&config);

        if accounts.genesis(&config, &mut genesis_state).is_err() {
            let state = genesis_state.checkpoint();
            return (Err(arbitrary::Error::IncorrectFormat), state);
        };

        let state = genesis_state.checkpoint();

        (Ok(accounts), state)
    }
}
