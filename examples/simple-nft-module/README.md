# How to Create a New Module Using the Module System

### Understanding the Module System

The Sovereign Software Development Kit (SDK) includes a [Module System](../../module-system/README.md),
which serves as a catalog of concrete and opinionated implementations for the rollup interface.
These modules are the fundamental building blocks of a rollup and include:

- **Protocol-level logic**: This includes elements such as account management, state management logic,
  APIs for other modules, and macros for generating RPC. It provides the blueprint for your rollup.
- **Application-level logic**: This is akin to smart contracts on Ethereum or pallets on Polkadot.
  These modules often use state, modules-API, and macros modules to simplify their development and operation.

### Creating a Non-Fungible Token (NFT) Module

**Note**: This tutorial focuses on illustrating the usage of the Sovereign SDK by creating a simple NFT module.
The focus here is on the module system and not the application logic. For a more complete NFT module, please refer
to [sov-nft-module](../../module-system/module-implementations/sov-nft-module)

In this tutorial, we will focus on developing an application-level module. Users of this module will be able to mint
unique tokens, transfer them to each other, or burn them. Users can also check the ownership of a particular token. For
simplicity, each token represents only an ID and won't hold any metadata.

## Getting Started

### Structure and dependencies

The Sovereign SDK provides a [module-template](../../module-system/module-implementations/module-template/README.md),
which is boilerplate that can be customized to easily build modules.

```text

├── Cargo.toml
├── README.md
└── src
    ├── call.rs
    ├── genesis.rs
    ├── lib.rs
    ├── query.rs
    └── tests.rs
```

Here are defining basic dependencies in `Cargo.toml` that module needs to get started:

```toml
[dependencies]
anyhow = { anyhow = "1.0.62" }
sov-modules-api = { git = "https://github.com/Sovereign-Labs/sovereign-sdk.git", branch = "stable", features = ["macros"] }
```

### Establishing the Root Module Structure

A module is a distinct crate that implements the `sov_modules_api::Module` trait. Each module
has private state, which it updates in response to input messages.

### Module definition

NFT module is defined as the following:

```rust
#[derive(sov_modules_api::ModuleInfo, Clone)]
pub struct NonFungibleToken<S: sov_modules_api::Spec> {
    #[id]
    id: sov_modules_api::ModuleId,

    #[state]
    admin: sov_modules_api::StateValue<S::Address>,

    #[state]
    owners: sov_modules_api::StateMap<u64, S::Address>,

    // If the module needs to refer to another module
    // #[module]
    // bank: sov_bank::Bank<S>,
}
```

This module includes:

1. **ID**: Every module must have a unique id, like a smart contract address in Ethereum.
2. **State attributes**: In this case, the state attributes are the admin's address and a map of token IDs to owner
   addresses.
   For simplicity, the token ID is an u64.
3. **Optional module reference**: This is used if the module needs to refer to another module.

### State and Context

#### State

`#[state]` values declared in a module are not physically stored in the module. Instead, the module definition
simply declares the _types_ of the values that it will access. The values themselves live in a special struct
called a `WorkingSet`, which abstracts away the implementation details of storage. In the default implementation, the actual state values live in a [Jellyfish Merkle Tree](https://github.com/penumbra-zone/jmt) (JMT).
This separation between functionality (defined by the `Module`) and state (provided by the `WorkingSet`) explains
why so many module methods take a `WorkingSet` as an argument.

#### Context

The `Context` trait allows the runtime to pass verified data to modules during execution.
Currently, the only required method in Context is sender(), which returns the address of the individual who initiated
the transaction (the signer).

Context also inherits the Spec trait, which defines the concrete types used by the rollup for Hashing, persistent data
Storage, digital Signatures, and Addresses. The Spec trait allows rollups to easily tailor themselves to different ZK
VMs. By being generic over a Spec, a rollup can ensure that any potentially SNARK-unfriendly cryptography can be easily
swapped out.

## Implementing `sov_modules_api::Module` trait

### Preparation

Before we start implementing the `Module` trait, there are several preparatory steps to take:

1.  Define `native` feature in `Cargo.toml` and add additional dependencies:

    ```toml
    [dependencies]
    anyhow = "1.0.62"
    borsh = { version = "0.10.3", features = ["bytes"] }
    serde = { version = "1", features = ["derive"] }
    serde_json = "1"

    sov-modules-api = { git = "https://github.com/Sovereign-Labs/sovereign-sdk.git", branch = "stable", default-features = false, features = ["macros"] }
    sov-state = { git = "https://github.com/Sovereign-Labs/sovereign-sdk.git", branch = "stable", default-features = false }

    [features]
    default = ["native"]
    serde = ["dep:serde", "dep:serde_json"]
    native = ["serde", "sov-state/native", "sov-modules-api/native"]
    ```

    This step is necessary to optimize the module for execution in ZK mode, where none of the RPC-related logic is
    needed.
    Zero Knowledge mode uses a different serialization format, so serde is not needed.
    The `sov-state` module maintains the same logic, so its `native` flag is only enabled in that case.

2.  Define `Call` messages, which are used to change the state of the module:

    ```rust
    // in call.rs
    #[cfg_attr(feature = "native", derive(serde::Serialize), derive(serde::Deserialize))]
    #[derive(borsh::BorshDeserialize, borsh::BorshSerialize, Debug, PartialEq, Clone)]
    pub enum CallMessage<S: sov_modules_api::Spec> {
        Mint {
            /// The id of new token. Caller is an owner
            id: u64,
        },
        Transfer {
            /// The address to which the token will be transferred.
            to: S::Address,
            /// The token id to transfer.
            id: u64,
        },
        Burn {
            id: u64,
        }
    }
    ```

    As you can see, we derive the `borsh` serialization format for these messages. Unlike most serialization libraries,
    `borsh` guarantees that all messages have a single "canonical" serialization, which makes it easy to reliably
    hash and compare serialized messages.

3.  Create a `Config` struct for the genesis configuration. In this case, the admin address and initial token distribution
    are configurable:

    ```rust
    // in lib.rs
    pub struct NonFungibleTokenConfig<S: sov_modules_api::Spec> {
        pub admin: S::Address,
        pub owners: Vec<(u64, S::Address)>,
    }
    ```

## Stub implementation of the Module trait

Plugging together all types and features, we get this `Module` trait implementation in `lib.rs`:

```rust, ignore
impl<S: sov_modules_api::Spec> Module for NonFungibleToken<S> {
    type Spec = S;
    type Config = NonFungibleTokenConfig<S>;
    type CallMessage = CallMessage<S>;

    fn genesis(
        &self,
        _config: &Self::Config,
        _state: &mut WorkingSet<S>,
    ) -> anyhow::Result<(), Error> {
        Ok(())
    }

    fn call(
        &self,
        _msg: Self::CallMessage,
        _context: &Context<Self::Spec>,
        _state: &mut impl TxState<S>,
    ) -> anyhow::Result<sov_modules_api::CallResponse, Error> {
        Ok(sov_modules_api::CallResponse::default())
    }
}
```

## Implementing state change logic

### Initialization

Initialization is performed by the `genesis` method,
which takes a config argument specifying the initial state to configure.
Since it modifies state, `genesis` also takes a working set as an argument.
`Genesis` is called only once, during the rollup deployment.

```rust, ignore
use sov_modules_api::WorkingSet;

// in lib.rs
impl<S: sov_modules_api::Spec> sov_modules_api::Module for NonFungibleToken<S> {
    type Spec = S;
    type Config = NonFungibleTokenConfig<S>;
    type CallMessage = CallMessage<S>;

    fn genesis(
        &self,
        config: &Self::Config,
        state: &mut WorkingSet<S>,
    ) -> Result<(), Error> {
        Ok(self.init_module(config, state)?)
    }
}

// in genesis.rs
impl<S: sov_modules_api::Spec> NonFungibleToken<S> {
    pub(crate) fn init_module(
        &self,
        config: &<Self as sov_modules_api::Module>::Config,
        state: &mut WorkingSet<S>,
    ) -> anyhow::Result<()> {
        self.admin.set(&config.admin, state);
        for (id, owner) in config.owners.iter() {
            if self.owners.get(id, state).is_some() {
                anyhow::bail!("Token id {} already exists", id);
            }
            self.owners.set(id, owner, state);
        }
        Ok(())
    }
}
```

### Call message

First, we need to implement actual logic of handling different cases. Let's add `mint`, `transfer` and `burn` methods:

```rust, ignore
use sov_modules_api::{event, WorkingSet};

impl<S: sov_modules_api::Spec> NonFungibleToken<S> {
    pub(crate) fn mint(
        &self,
        id: u64,
        context: &Context<S>,
        state: &mut WorkingSet<S>,
    ) -> anyhow::Result<sov_modules_api::CallResponse> {
        if self.owners.get(&id, state).is_some() {
            bail!("Token with id {} already exists", id);
        }

        self.owners.set(&id, context.sender(), state);

        self.emit_event(state, Event::Mint { id });
        Ok(sov_modules_api::CallResponse::default())
    }

    pub(crate) fn transfer(
        &self,
        id: u64,
        to: S::Address,
        context: &Context<S>,
        state: &mut WorkingSet<S>,
    ) -> anyhow::Result<sov_modules_api::CallResponse> {
        let token_owner = match self.owners.get(&id, state) {
            None => {
                anyhow::bail!("Token with id {} does not exist", id);
            }
            Some(owner) => owner,
        };
        if &token_owner != context.sender() {
            anyhow::bail!("Only token owner can transfer token");
        }
        self.owners.set(&id, &to, state);
        self.emit_event(state, Event::Transfer { id });
        Ok(sov_modules_api::CallResponse::default())
    }

    pub(crate) fn burn(
        &self,
        id: u64,
        context: &Context<S>,
        state: &mut WorkingSet<S>,
    ) -> anyhow::Result<sov_modules_api::CallResponse> {
        let token_owner = match self.owners.get(&id, state) {
            None => {
                anyhow::bail!("Token with id {} does not exist", id);
            }
            Some(owner) => owner,
        };
        if &token_owner != context.sender() {
            anyhow::bail!("Only token owner can burn token");
        }
        self.owners.remove(&id, state);

        self.emit_event(state,  Event::Burn { id });
        Ok(sov_modules_api::CallResponse::default())
    }
}
```

And then make them accessible to users via the `call` function:

```rust, ignore
impl<S: sov_modules_api::Spec> sov_modules_api::Module for NonFungibleToken<S> {
    type Spec = S;
    type Config = NonFungibleTokenConfig<S>;

    fn call(
        &self,
        msg: Self::CallMessage,
        context: &Context<Self::Spec>,
        state: &mut impl TxState<S>,,
    ) -> Result<sov_modules_api::CallResponse, Error> {
        let call_result = match msg {
            CallMessage::Mint { id } => self.mint(id, context, state),
            CallMessage::Transfer { to, id } => self.transfer(id, to, context, state),
            CallMessage::Burn { id } => self.burn(id, context, state),
        };
        Ok(call_result?)
    }
}
```

### Enabling Queries

We also want other modules to be able to query the owner of a token, so we add a public method for that.
This method is only available to other modules: it is not currently exposed via RPC.

```rust, ignore
use jsonrpsee::core::RpcResult;
use sov_modules_api::macros::rpc_gen;
use sov_modules_api::{Context, WorkingSet};

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
/// Response for `getOwner` method
pub struct OwnerResponse<S: Spec> {
    /// Optional owner address
    pub owner: Option<S::Address>,
}

#[rpc_gen(client, server, namespace = "nft")]
impl<S: sov_modules_api::Spec> NonFungibleToken<S> {
    #[rpc_method(name = "getOwner")]
    pub fn get_owner(
        &self,
        token_id: u64,
        state: &mut WorkingSet<S>,
    ) -> RpcResult<OwnerResponse<S>> {
        Ok(OwnerResponse {
            owner: self.owners.get(&token_id, state),
        })
    }
}
```

## Testing

Integration tests are recommended to ensure that the module is implemented correctly. This helps confirm
that all public APIs function as intended.

Temporary storage is needed for testing, so we enable the `temp` feature of `sov-state` as a `dev-dependency`.
Implementation of SnapshotQuery is also needed, so `sov-prover-storage-manager` is also added.

```toml,text
[dev-dependencies]
sov-state = { git = "https://github.com/Sovereign-Labs/sovereign-sdk.git", branch = "stable", features = ["temp"] }
sov-prover-storage-manager = { git = "https://github.com/Sovereign-Labs/sovereign-sdk.git", branch = "stable" }
```

Here is some boilerplate for NFT module integration tests:

```rust
use simple_nft_module::{CallMessage, NonFungibleToken, NonFungibleTokenConfig, OwnerResponse};
use sov_modules_api::{Address, Context, Module, WorkingSet};
use simple_nft_module::Event;
use sov_state::{DefaultStorageSpec, ProverStorage};

pub type S = sov_test_utils::TestSpec;
pub type Storage = ProverStorage<DefaultStorageSpec<sov_test_utils::TestHasher>>;


#[test]
#[ignore = "Not implemented yet"]
fn genesis_and_mint() {}

#[test]
#[ignore = "Not implemented yet"]
fn transfer() {}

#[test]
#[ignore = "Not implemented yet"]
fn burn() {}
```

Here's an example of setting up a module and calling its methods:

```rust
#[test]
fn transfer() {
    // Preparation
    let admin = generate_address::<S>("admin");
    let admin_context = Context::<S>::new(admin.clone(), 1);
    let owner1 = generate_address::<S>("owner2");
    let owner1_context = Context::<S>::new(owner1.clone(), 1);
    let owner2 = generate_address::<S>("owner2");
    let config: NonFungibleTokenConfig<S> = NonFungibleTokenConfig {
        admin: admin.clone(),
        owners: vec![(0, admin.clone()), (1, owner1.clone()), (2, owner2.clone())],
    };
    let mut state = WorkingSet::new(ProverStorage::temporary());
    let nft = NonFungibleToken::new();
    nft.genesis(&config, &mut state).unwrap();

    let transfer_message = CallMessage::Transfer {
        id: 1,
        to: owner2.clone(),
    };

    // admin cannot transfer token of the owner1
    let transfer_attempt = nft.call(transfer_message.clone(), &admin_context, &mut state);

    assert!(transfer_attempt.is_err());
    // ... rest of the tests
}
```

## Plugging in the rollup

Now this module can be added to rollup's `Runtime`:

```rust, ignore
use sov_modules_api::{DispatchCall, Genesis, MessageCodec};

#[derive(Genesis, DispatchCall, MessageCodec)]
#[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
pub struct Runtime<S: sov_modules_api::Spec> {
    #[allow(unused)]
    sequencer: sov_sequencer_registry::Sequencer<S>,

    #[allow(unused)]
    bank: sov_bank::Bank<S>,

    #[allow(unused)]
    nft: simple_nft_module::NonFungibleToken<S>,
}
```
