- #956 Split stf_blueprint into smaller chunks.
- #950 Add metadata abou gas & fees to the serialized proof.
- #943 Add `ProofSerializer` trait which allow adding additional metadata to the proof blob.
- #908 refactors the testing framework to achieve the following objectives:
   - Allow the execution of transactions from different modules in one test
   - Allow the execution of multiple batches within one slot
- #927 Refactor common fields in `Transaction`, `UnsignedTransaction` & `Message` into a new `TxDetails` struct.
- #913 Read prover address from file configuration file.
- #910 replaces `constants.json` with `constants.toml` and `constants.test.json` with `constants.testing.toml`. The expected file "schema" and contents are the same, just the file format has changed and you'll just need to translate your file contents from JSON to TOML.
- #906 Simplifies the `UnmeteredStateWrapper` wrapper struct and adds it in the testing framework as a wrapper of the `TxState` trait to prevent test maintainers from charging gas in the hooks.
-  #889 Add `rewarded_addresses` filed to `openapi::AggregatedProofPublicData` 
-  #869 Add prover address to `AggregatedProofPublicData` 
- #904 removes the need to specify event keys when calling `self.emit_event`, because the event key is now generated automatically based on the module name and the event variant (e.g. `Bank/TokenCreated`). You can use `self.emit_event_with_custom_key` to emit events with custom keys.
- #900 Track & charge pre-execution gas costs during direct sequencer registration.
- #881 adds blessed constants in the `constants.toml` to be used to charge gas in the tests. It also removes an unused method `tx_fixed_cost` that got deprecated with the recent changes in gas. The default value of the `MAX_FEE` in the CLI wallet is now `10_000_000`.
- #864 Add prover address to `AggregatedProofPublicData` 
- #885 renames the `rpc.rs` file in the standard SDK module template to `query.rs`.
- #869 Add prover address to `AggregatedProofPublicData` 
- #862 Add `prover_address` to `StateTransitionPublicData` this will allow rewarding the prover.
- #859 Adds a `MeteredSignature` struct wrapper and a `MeteredBorshDeserialize` trait that respectively charges gas for signature verification and borsh deserialization. We have renamed the `hash.rs` file to `metered_utils.rs` inside the `sov-modules-api/common` crate and grouped all the custom metered utils there. 
- #863 Upgrades borsh to version 1.0. If you import borsh in one of your crates, be sure to upgrade as well.
- #856 removes support for the `ledger_*` RPC methods and replaces them with a REST API accessible by default at `http://localhost:12346/ledger`.
- #862 Adds `StateTransitionWitnessWithAddress` struct that is used to pass the prover address to the Prover.
- #853 Make the `ProverIncentives` call methods public and accept prover address explicitly.
- #823 Adds StorableMockDaService, where blocks will be persisted between restarts. `[da]` section for adapter has been changed. 
- #821 removes support for the RPC methods of rollup sequencers, and replaces them with a REST API accessible by default at `http://localhost:12346/sequencer`.
- #851 Add BlobData constructors. 
- #842 `ProofManager` saves zk-proofs received from the `STF` in the db. 
- #839 Refine "ProofProcessor" capability.
- 835 adds a new `MeteredHasher` struct that charges gas for every hash computation. This structure is meant to be used in the module system to charge gas when hashing data.
- #835 adds a new `MeteredHasher` struct that charges gas for every hash computation. This structure is meant to be used in the module system to charge gas when hashing data.
- #828 add `ProofProcessor` capability.
- #813 is a follow-up of #783, it sets the constants that define the costs for gas access to non-zero values. That way, we charge some gas when trying to access the storage and the metered accessors (like the `WorkingSet`) can now run out of gas because of state accesses.
Meaningful changes:
  - We had to change the blessed values for the transaction fee, the initial account balances and the attester/prover stakes. The new values are: `MAX_FEE = 1_000_000`, `INITIAL_BALANCE = 1_000_000_000` and `DEFAULT_STAKE = 100_000`.
  - We had to replace the gas computation of some tests that made strong assumptions about the gas consumed by transaction execution.
- #814 mandates `0x` prefixes for hashes in RPC and REST APIs, whereas before it was optional.
- #809 extend blob-storage to support proof DA namespace.
- #806 simplify new_test_blob_from_batch
- #783 enables gas metering for storage accesses inside the module system. In particular, it makes all the state accessors fallible, and the module code now have to handle the case where the state accessor runs out of gas.
Meaningful changes
  - Making all the state accessors fallible: for instance `state_value.get(&mut state_reader)` now returns `Result<Option<StateValue>, StateReader::Error>`. Depending on the type of the state reader, the error type may be `Infallible` (for unmetered state accessors) or `StateAccessorError` (for metered state accessors)
  - The metered state accessors are: `WorkingSet` and `PreExecWorkingSet` when accessing the provable state; the unmetered state accessors are `TxScratchpad`, `StateCheckpoint`, `KernelWorkingSet`, ... 
  - A summary of all the access control patterns can be found in `sov-modules-api/src/state/accessors/access_controls.rs`
  - A new cargo dependency has been added: `unwrap_infallible`. This allows us to safely unwrap the type `Error<T, Infallible>` which has become ubiquitous because the `Infallible` error type is raised whenever an unmetered state accessor tries to access the state (416 occurrences in our codebase after this PR is merged).
  - The `WorkingSet` use, specially its instantiation with the `WorkingSet::new` pattern has been *significantly* reduced in favor of using the `StateCheckpoint` in the tests. Since the `StateCheckpoint` is an unmetered state accessor, this makes the tests more manageable and maintainable - we can use `unwrap_infallible`. Besides, one now needs to use a special `GenesisStateAccessor` to instantiate the modules at genesis, which needs to be built from the `StateCheckpoint`.
  - temporary `evm` feature that allows converting the `WorkingSet` to an `UnmeteredStateWrapper`. **This is a temporary solution and that feature will be removed once we find a way to connect the EVM and module gas metering**. This is the final task of https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/734.

- #800 `LedgerDb` explicitly returns changes instead of saving them in local cache of underlying `CacheDb`.
- #796 Reuse Batch struct in BatchWithId & PreferredBatch.
- #789 Changes the Transaction receipt type to include the reason a transaction was skipped or reverted. Consumers of the API should now use the `sov_stf_blueprint::TxEffect` as the `TxReceipt` return type for ledger RPC queries.
- #790 Simplify `BlobSelector` code.
- #753 updates default `max_fee` in `sov-cli` to `10_000` (from previous value of `0`)
- #791 adds `Self::TransactionId` to `DaService::send_aggregated_zk_proof`.
- #787 split capabilities into separate modules.
- #770 enforces that transactions are set with the correct chain ID. To get the ID, use `config_value!("CHAIN_ID")`.

- #775 adds a couple of custom error types that will be useful to allow using the `try` pattern within the module system to automatically convert errors into `anyhow::Error`. Meaningful changes:
  - Adding a custom `GasMeteringError` enum that describes the possible causes for error inside the `GasMeter` trait methods
  - Adding a custom `StateAccessorError` enum that describes the situations where accessing the state might fail.
  - Adding the `std::error::Error + Send + Sync` bounds to the associated `Error` type in `StateReader` and `StateWriter` (this allows automatic conversion to `anyhow::Error`
- #766 modifies the RPC interface to accept an `ApiStateAccessor` instead of a `WorkingSet` to prepare the full integration of the gas metering for state accesses. In particular this commit changes the `RPC` macro to accept an `ApiStateAccessor` instead of a `WorkingSet` as an argument to the rpc methods.
- #726 adds Swagger UI -an OpenApi playground- as an endpoint defined inside `sov_modules_stf_blueprint::Runtime::endpoint`.
- #743 adds metered state accessor traits to the `sov-modules-api/state` module.
- #764 changes notifications logic, so ledger notifications arrive after state has been completely updated.
- #750 moves `RuntimeAuthenticator, RuntimeAuthorization, and Authenticator` to a separate file in the capabilities module.
- #751 adds `DaServiceWithRetries` wrapper.
   - To use it, make sure your `DaService::Error` type is `MaybeRetryable<anyhow::Error>`. You can then wrap your `DaService` in `DaServiceWithRetries` whilst providing a policy for exponential retries via: `DaServiceWithRetries::with_exponential_backoff(<da-service>, <exponential-backoff-policy>)`. This wrapped `DaService` can then be used normally as a `DaService` anywhere.
   - Use the `MaybeRetryable` return type in your `DaService` to indicate whether a fallibe function maybe retried or not, by returning either `MaybeRetryable::Transient(e)` if you want your functions to be retried in the case of errors, or `MaybeRetryable::Permanent(e)` if you don't want them to be retried.
- #749 makes the rollup generic over the `AuthorizationData` type.
- #742 moves the scratchpad and the state into a submodule of the sov-modules-api and breaks these files into smaller components to make the complexity more manageable.
- #730 Splits `RollupBlueprint` into two traits: `FullNodeBlueprint` and `RollupBlueprint` and feature gates the full-node blueprint behind the `"native"` feature flag. It also reduces the number of required types for the `RollupBlueprint` by making it generic over execution mode. See the diff of `celestia_rollup.rs` for a complete example of a migration.
- #735 updates `minter_address` to `mint_to_address` in `CallMessage::CreateToken` & `CallMessage::Mint`
- #725 removes the `macros` feature from `sov-modules-api`, which is now always enabled even with `--no-default-features`.
- #732 adds the `sov-nonces` module.
- #714 integrates a batch of changes to the `StfBlueprint` and the capabilities. Meaningful changes:
  - Remove arguments of type `SequencerStakeMeter` from the capabilities and replace all of the `(SequencerStakeMeter, StateCheckpoint)` couple of variables by a single `PreExecWorkingSet` which is a type safe data structure that should charge for gas before transaction execution starts.
  - Removing the `ExecutionMode` type from the `StfBlueprint`. It is now replaced by `TxScratchpad`, which is an intermediary type between `WorkingSet` and `StateCheckpoint`. This is useful for the `sov-sequencer` crate because one may want to revert all the changes that happened during transaction execution when simulating a transaction before adding it to a sequencer batch.
  - The entire transaction processing lifecycle is now embedded in the `process_tx` method that is called by both the `apply_batch` in `StfBlueprint` and `try_add_tx_to_batch` in the `sov-sequencer` crate. Hence we now have the following state machine for transaction execution:
        - In `to_tx_scratchpad` we convert the `StateCheckpoint` into a revertable `TxScratchpad` for transaction execution.
        - In `authorize_sequencer` we consume the `TxScratchpad` and build a `PreExecWorkingSet` on success, we stop processing the batch on failure
        - In `try_reserve_gas` we consume the `PreExecWorkingSet` and we build a `WorkingSet` on success (transaction execution starts), or return a `PreExecWorkingSet` on failure (so that we can penalize the sequencer).
        - In `attempt_tx` we consume the `WorkingSet` and output a `TxScratchpad`
        - The `penalize_sequencer` method consumes a `PreExecWorkingSet` and outputs a `TxScratchpad`
  - The `SequencerTxOutcome` type is removed. Now all pre-execution capabilities are processed in `process_tx`. When entering `apply_tx`, the sequencer cannot be penalized anymore.

- #699 adds tests for the `EVM` credentials.
- #717 replaces manual serialization & deserialization for the signed parts of `Transaction` struct by creating a new `UnsignedTransaction` object that implements Borsh traits. The breaking change is that the bytes over we sign now include an additional vector length for the runtime message field.
- #700 adds an associated `TxState` type to the `TxHooks` trait and uses it as an argument in place of a concrete `WorkingSet`.
- #563 introduces a REST API for modules and runtimes. The `RollupBlueprint::create_endpoints` method in `demo-rollup` has been updated accordingly, so it exposes both JSON-RPC and the REST API by default. You can find the documentation for generating a REST API in `sov_modules_api::rest`.
- #696 requires `Runtime` implementers to implement the `HasCapabilities` trait, which allows the `Runtime` to delegate to another struct for much of its required functionality. If a `Runtime` does not wish to delegate, it can simply implement the trait with `Self` as the associated `Capabilities` type. Implementations can be found in the sov-capabailities crate.
- #701 adds support for multiple credentials in `sov-accounts`. This is a breaking change for the consumers of the SDK.
- #694 adds `Credentials` to the `Context` structure. This is a breaking change for the consumers of the SDK. See implementation of the `RuntimeAuthorization::resolve_context` method.
- #688 follow-up of https://github.com/Sovereign-Labs/sovereign-sdk-wip/pull/681 that modifies the gas interface to prevent access to `TxGasMeter` outside of `sov-modules-api`.
The spirit of this change is to increase the coupling between the `WorkingSet` and `Transaction` types. Meaningful changes:
  - Change the visibility of `TxGasMeter` to `pub(crate)`.
  - Change the `WorkingSet` interface to return `TransactionConsumption` instead of `GasMeter` when consumed with `revert` or `checkpoint`.
  - Move the `TransactionConsumption` and `SequencerReward` types to `scratchpad`
  - Change the `GasEnforcer` capability to handle the gas workflow without having access to `TxGasMeter`. In particular change the `consume_gas_and_allocate_rewards` capability to the simpler `allocate_consumed_gas` which distributes the transaction reward returned when calling `checkpoint` on the `working_set` to the base fee and tip recipient.
  - Remove access to methods that can artificially modify the gas meter outside of testing (like methods to build `unmetered` gas meters).

- #683 replace `Public Key Hash` with more general concept of `CredentialId`. This is a breaking change for the consumers of the SDK.
- #689 makes `#[rpc_gen]` annotations on modules optional, so you can safely remove it if you don't need it. Additionally, the `get_rpc_methods` function generated by `#[expose_rpc]` now doesn't depend on the import of e.g. `BankRpcImpl` and you should these imports.
- #683 replace `Public Key Hash` with more general concept of `CredentialId`. This is a breaking change for the consumers of the SDK.
- #686 Changes the API of `Module::genesis` to accept an `&mut impl sov_modules_api::GenesisState<S>` instead of a concrete type.
- #679 remove the transaction signature check from the `EVM` module.
- #681 contains some interface improvements for the gas which become possible after `sov-core` and `sov-api` got merged. In particular:
    - Move `TxGasMeter` from `common/gas` to `transaction` which allows more type-safety by removing methods to create and modify arbitrary gas meters, tying it to the `AuthenticatedTransactionData` type.
    - Remove `Tx` and `TxGasMeter` associated types from the `GasEnforcer` trait.
- #647 completes the gas workflow for the StfBlueprint by enhancing the interface by adding some type-safety guarantees to the StfBlueprint and simplifying the penalization workflow. Follow-up of #619.
    - Added a new `consume_gas_and_allocate_rewards` capability to the GasEnforcer to allocate transaction rewards at the end of the transaction execution. This was previously done in `refund_remaining_gas`
    - Added a new type `TransactionConsumption` that tracks the amount of gas consumed and can only be built using the `AuthenticatedTransactionData` and by consuming the associated `TxGasMeter`.
    - `refund_remaining_gas` can only be called either after `consume_gas_and_allocate_rewards` or with a zero `TransactionConsumption` (speculative case for reverted transactions)
    - after `consume_gas_and_allocate_rewards` the `GasMeter` is consumed and cannot be used anymore
- #673 Removes `std` feature from `rollup-interface` and `no_std` support. usage of `sov_rollup_interface::maybestd` should be changed back to `std`.
- #680 Extends sov-cli:
    - Adds new optional boolean parameter to `submit-batch`, that tells sov-cli to wait for batch to be processed by full node
    - set url now expects second parameter for REST API endpoint.
- #663 Modifies the interface of traits `RuntimeAuthenticator` and `RuntimeAuthorization`. Associated types `Tx` and `Gas` have been removed. `RuntimeAuthenticator` is now generic over `S: Spec`. Methods' type signatures have been slightly modified; please see `examples/demo-rollup/stf/src/authentication.rs` for an example on the new usage.
- #633 Deprecate `sov-modules-core`, move definitions into `sov-modules-api` & `sov-state`
- #664 removes the `Transaction` wrapping in `sov-ethereum` for EVM transactions. This is a breaking change for consumers of the SDK. See `RuntimeAuthenticator::authenticate`.
- #646 adds authenticator dispatch logic in`RuntimeAuthenticator::authenticate`.
- #613 Makes `sov_state::Storage` trait to be immutable and explicitly produce changes. SimpleStorageManager should be used when data needs to be persisted between batches.
- #631 removes the need for modules to `#[derive(ModuleCallJsonSchema)]`; the trait is automatically blanket-implemented for all modules as long as `CallMessage` implements `schemars::JsonSchema`.
- #628 all the account resolution logic was moved to `resolve_context`. This method now returns a `Result<Context, _ >` instead of a `Context`. This is a breaking change for consumers of the SDK.
- #621 removes the need for a prelude `sov_modules_api::prelude` which re-exposes a few common types for convenience, as well as external crates like `clap` and `serde_json` (for now, more will follow). You can remove these dependencies from your `Cargo.toml` if you wish.
- #620 Adds more fields to the `Event`s emitted by the `sov-bank` module. Start emitting events for token minting.
- #619 starts charging gas for signature checks in the StfBlueprint and completes the refactoring effort started in #612. There was the following changes in the interface:
  - Introduction of a `GasMeter` trait and the three associated implementations: `TxGasMeter` (what used to be the `GasMeter` struct), `UnlimitedGasMeter` (a gas meter that holds an infinite reserve of gas) and the `SequencerStakeMeter` (a gas meter specially designed to track the sequencer stake and accumulate penalties).
  - Adding the `sequencer_stake_meter` as an argument of the `authenticate` method of the `RuntimeAuthenticator` (as an associated type) and the `Authenticator` (as a `&mut impl GasMeter` in that case).
  - Adding the `refund_sequencer` capability which can be used to refund the sequencer some of the penalties he accumulated during the pre-execution checks.
  - Modify the `authorize_sequencer` and `penalize_sequencer` capabilities to take the `SequencerGasMeter` as a parameter instead of a fixed amount. This allows type safety and removes the unsafe `saturating_sub` in the implementation of `penalize_sequencer`.
  - Add a `TxSequencerOutcome` which is an enum with the variants `Rewarded(amount)` and `Penalized`.
  - Rename `SequencerOutcome` to `BatchSequencerOutcome` to represent the `SequencerOutcome` following batch execution.

- #622 Make `DefaultStorageSpec` generic over a `Hasher` instead of defaulting to `Sha256`
- #622 Make `DefaultStorageSpec` generic over a `Hasher` instead of defaulting to `Sha256`
- #623 updates the AccountConfig structure. This change is breaking for consumers of the SDK, as the format of the accounts.json configuration files has been changed.
- #617 adds a `gas_estimate` method to the DA layer `Fee` trait
- #614 Change the final argument of `Module::call` from a concrete `WorkingSet` to an abstract `impl TxState` type. It also removes the `working_set.accessory_state()` method and grants direct access to that state through the `WorkingSet` (read/write) and `impl TxState` (read-only).
- #612 refactors the `StfBlueprint` to deserialize, perform signature checks and execute transactions in one pass instead of doing pre-checks for the entire batch followed by the batch execution. Simplifies and fix the sequencer reward workflow. In particular:
  - Replace the previous mechanism that was using a mutable `i64` typed, `sequencer_reward` variable that accumulated the reward/penalties for the sequencer in the entire batch. This mechanism was buggy and did not properly account for side effects (e.g when the sequencer gets penalized more than his current stake, his transactions shouldn't be processed anymore).
  - Augment the authorization error for the `rawTx` verification with an enum with 2 variants: `FatalError` (sequencer gets slashed for acting maliciously) and `Invalid` (sequencer gets penalized a constant amount from his balance).
  - Remove the `Penalized` variant from the `SequencerOutcome`. This is replaced by the `penalize_sequencer` capability that is called whenever the sequencer gets penalized.
  - Add a `authorize_sequencer` capability that verifies that the sequencer has locked enough tokens to execute the next transaction. Since the sequencer bond can vary between transactions (because he may get penalized), this method needs to be called whenever the sequencer start executing a new transaction.
- #599 changes the way you define Rust constants which use the `constants.toml` file.
  Instead of the attribute macro `#[config_constant]`, you'll now use `config_value!`, like this:

  ```rust
  // Before
  #[config_constant]
  pub(crate) const GAS_TX_FIXED_COST: [u64; 2];

  // After
  pub(crate) const GAS_TX_FIXED_COST: [u64; 2] = config_value!("GAS_TX_FIXED_COST");

  // For Bech32:
  // Before
  #[config_bech32_constant]
  const TEST_TOKEN_ID: TokenId;

  // After
  const TEST_TOKEN_ID: TokenId = config_bech32!("TEST_TOKEN_ID", TokenId);
  ```

  Read the PR description for more details.

- #586 Adds a second Zkvm generic to the `StateTransitionFunction` API. This VM is used for generation of *`Aggregate`* zk proofs,
while the first VM continues Ato be used for block production. The signature of the `StfVerifier<DA, Vm, ZkSpec, RT, K>` was also changed to `StfVerifier<DA ZkSpec, RT, K, InnerVm, OuterVm>`

- #584 removes support for the `DefaultRuntime` derive macro. You must replace all proc-macro invocations of `DefaultRuntime` with `#[derive(Default)]`.
- #590 upgrade rustc version to 1.77. Installation new risc0 toolchain is needed. Simply
  run `make install-risc0-toolchain`
- #584 removes support for the `DefaultRuntime` derive macro. You must replace all proc-macro invocations
  of `DefaultRuntime` with `#[derive(Default)]`.

- #572 adds a new `da_service` method `estimate_fee()` and requires all blob submission methods to provide a `fee` as an argument.
- #580 `sov-cli` now returns an error on duplicate nickname key import or generation.
- #572 adds a new `da_service` method `estimate_fee()` and requires all blob submission methods to provide a `fee` as an
  argument.
- #573 Changes the `Bank::create_token signature`, allowing modules to create tokens.

- #547 removes the `From<[u8;32]>` bound on rollup addresses and replaces it with a bound expressing that for
  any `Spec`, the `Address` type must implement `From<&PublicKey>`. It also removes the nft module's `CollectionAddress`
  type and replaces it with a 32-byte `CollectionId`.
- #546 fixes a bug where sequencer could protect themselves from slashing by exiting while their batch was being
  processed.

- #545 moves the `RelevantBlobs`, `RelevantBlobIters`, and `DaProof` types from `rollup_interfaces::da::services`
  to `rollup_interfaces::da`. It also adds a feature gate to require the `native` flag to
  use `rollup_interfaces::da::services`.

- #535 fixes the sequencer tip calculation in the `StfBlueprint`. This calculation now takes into account the changes of
  the #429 (which fixes the gas reserve calculation). Also renames `max_priority_fee` to `max_priority_fee_bips` in
  the `Transaction` structure.

- #528 removes the `initial_base_fee_per_gas` parameter from the genesis configuration of the chain state to define a
  constant `INITIAL_BASE_FEE_PER_GAS` that is common to all crates. Now that we assume the tx sender always has a bank
  account for the gas token, the non-zero transfer amount hacks from the `reserve_gas` and `refund_remaining_gas`
  capabilities for the `GasEnforcer` in the `Bank` module are replaced by an account check. If the sender doesn't have a
  bank account for the gas token, the method fails with the error `AccountDoesNotExist`. This check is done in the
  EIP-1559 specification.
- #519 Adds an `Authenticator` trait which abstracts away the transaction authentication logic. This is a breaking
  change so the consumers of the SDK will need to implement the new trait.
  See `authentication.rs` in `demo-rollup`.

- #538 removes the chain state from the EVM module because it was unused.

- #528 removes the `initial_base_fee_per_gas` parameter from the genesis configuration of the chain state to define a
  constant `INITIAL_BASE_FEE_PER_GAS` that is common to all crates. Now that we assume the tx sender always has a bank
  account for the gas token, the non-zero transfer amount hacks from the `reserve_gas` and `refund_remaining_gas`
  capabilities for the `GasEnforcer` in the `Bank` module are replaced by an account check. If the sender doesn't have a
  bank account for the gas token, the method fails with the error `AccountDoesNotExist`. This check is done in the
  EIP-1559 specification.

- #526 `Bank::create_token` & `Bank::mint` now accept an `impl Payable` as arguments. This is a breaking
  change. `&S::Address` already implements `Payable`, and `ModuleId` can be promoted to `Payable`
  via `ModuleId::as_token_holder`.

- #525 mitigates the bug where a transaction which was included multiple times would always return the `Duplicate` tx
  status rather than returning info about the original tx execution. In this commit we use a simple heuristic (first tx
  wins) to guess which instance is the "correct" one.


- #468 fixes the gas elasticity computation that was removed in #476. The base fee computation is now done in
  the `ChainState` module as part of the `begin_slot` hook. This PR also updates the `ChainState` integration tests to
  check that the base fee computation is correctly performed in the module hooks. It also adds gas elasticity tests for
  the `ChainState` module.

- #490 Adds a `inner_code_commitment`, an `outer_code_commitment` and a `initial_da_height` field to the `ChainState`
  module. These fields should be initialized at genesis. Added getters for `genesis_da_height`, `outer_code_commitment`
  and `inner_code_commitment` in the `ChainState` module. Adapted the `demo-rollup` json configurations to use these
  fields. Added a new configuration folder for the stf tests that rely on `MockCodeCommitment` which have a different
  format from the `risc0` code commitments (32 bytes instead of 8). Modified the `AttesterIncentives` module to use
  the `inner_code_commitment` field from the `ChainState` module instead of
  the `commitment_to_allowed_challenger_method` field. Modified the `ProverIncentives` module to use
  the `outer_code_commitment` field from the `ChainState` module instead of the `commitment_of_allowed_verifier_method`
  field.

- #487 Introduces the `AuthenticatedTransactionData` structure. This is the transaction data that passed the
  authentication phase. And updates `Accounts::CallMessage` format. This is a breaking change for consumers of the SDK
  only if they send messages directly to the Accounts module.

- #484 Adds a new `CodeCommitment` trait and applies it to the associated type of the ZKVM. The
  trait provides encode/decode methods, in addition to all existing functionality. These methods
  should be used to convert to/from the `code_commitment` vector in `AggregatedProofPublicData`.

- #480 The `Accounts` module now keeps PublicKey hashes instead of PublicKeys. This is a breaking change for consumers
  of the SDK only if they send messages directly to the Accounts module.

- #479 refactors the `ChainState` module integration test to be more readable and less repetitive.

- #476 updates the gas interface for the ChainState module, removes the gas price elasticity computation (it will be
  fixed in #468) and propagates these changes throughout the infrastructure.
  Meaningful changes:
    - Added the `INITIAL_GAS_LIMIT` and `initial_gas_price` (defined at genesis) constants. These constants are defined
      in the EIP-1559 and are used to handle the gas lifecycle in the chain-state module
    - Rename the `gas_price` (a generic name not used anywhere in the EIP-1559) to `base_fee_per_gas` which is the
      official naming for this variable
    - Create a `BlockGasInfo` structure that groups the `gas_used`, `gas_limit` and `base_fee_per_gas` into one wrapper.
    - Removed the `gas_price_state` from the `chain-state` module's state. There was multiple reasons behind that:
    - Removed the outdated gas elasticity mechanism

- #481 This PR combines the `ContextResolver` and `TransactionDeduplicator` traits into a single `RuntimeAuthorization`
  trait. This is a breaking change, and consumers of the SDK will need to implement the new trait.

- #472 This PR breaks downstream code in the following way:
  `PublicKey::to_address` is now parameterized by `Hasher`.

- #471 adds 3 new parameters to sov-demo-rollup
    - optional cmd `--genesis-config-dir ../test-data/genesis/demo/celestia` to specify the genesis config directory
    - optional cmd `--prometheus_exporter_bind 127.0.0.1:9845` to specify the prometheus exporter bind address. Useful
      for running several nodes on the same host.
    - environment `export SOV_TX_SIGNER_PRIV_KEY_PATH=examples/test-data/keys/tx_signer_private_key.json` to specify the
      path to the transaction signer private key.

- #452 abstracts away the transaction authorization logic. The consumers of the `sov-module-api` have to implement the
  new `RuntimeAuthenticator` trait. Refer to `hooks_impl.rs` for details

- #413 introduces new RESTful JSON APIs for the sequencer and, most importantly, modifies the `RollupBlueprint` trait
  interface to allow implementations to expose Axum servers, instead of only JSON-RPC servers. In
  fact, `RollupBlueprint::create_rpc_methods` was renamed to `RollupBlueprint::create_endpoints`, which returns a tuple.
  Most `RollupBlueprint` implementations will need to use the new `sov_modules_rollup_blueprint::register_endpoints`,
  which replaces `sov_modules_rollup_blueprint::register_rpc`. Take a look at how `examples/demo-rollup` implements the
  new interface to see how it works.

- #439 Implements the `SequencerRegistry` module to support Sequencers' reward and penalties. In particular,
  the `SequencerRegistry` can now be used in conjunction with the `GasEnforcer` capability hook to reward the
  sequencer for submitting a correct transaction.

- #444 Moves the tests for the `SequencerRegistry` module to the `src` directory of the same crate.

- #443 Removes the `coins` field in the `SequencerRegistry` struct. It is replaced by a `minimum_bond` field and
  the `TokenId` becomes `GAS_TOKEN_ID`. The configuration structure `SequencerRegistryConfig` should be updated to
  replace the `coin` field by the new `minimum_bond` field.

- #432 Updates the `StateTransitionFunction`  to handle blobs from all the relevant namespaces.
  This breaks the `StateTransitionFunction` API but the breaking changes don't propagate outside of the module system
  internals.

- #441 Removes the section of the `rollup_cofing.toml` called `[prover_service]` and moves its existing value to a
  section called `[proof_manager]`. To update, it's sufficient to simply search and replace `[prover_service]`
  to `[proof_manager]` in any configuration files.

- #429 Updates the `reserve_gas` and `refund_remaining_gas` mechanisms to match EIP-1559. The `reserve_gas`
  and `refund_remaining_gas` methods are moved back to the `Bank` module as they now affect multiple modules (the module
  that locks the gas tip - ie the `sequencer-registry` - and the module that locks the base gas - ie
  the `attester-incentives` or `prover-incentives`). Instead of locking the gas in
  the `attester-incentives`, `prover-incentives` or `sequencer-registry` at the `reserve_gas` call, we are now doing it
  when `refund_remaining_gas` is called. The `Transaction` structure is updated to let the user specify a `max_fee` and
  a `max_priority_fee` which are respectively a coin amount and a percentage. He may optionally specify a `gas_limit`
  which is a multi-dimensional gas limit that is used as a protection for gas elasticity (following EIP-1559).

- #425 Updates the `CelestiaVerifier` to support multiple namespaces. This change is breaking for consumers of
  the `Sovereign-sdk`: The `CelestiaVerifier` now needs to be initialized with `ROLLUP_BATCH_NAMESPACE`
  and `ROLLUP_PROOF_NAMESPACE`. See:
    1. https://github.com/Sovereign-Labs/sovereign-sdk-wip/pull/425/files#diff-75e27b2869f342897e1c89ed4abe7ff82ce8368a795dbefdffac8e30bbcb11f4L36

    2. https://github.com/Sovereign-Labs/sovereign-sdk-wip/pull/425/files#diff-d46bdfc6e8e6dfb4acd9794c4536d6a8212b37aef27abc4b39d7db479be75d4aL135

- #406 Updates the `DaService` trait and the `Celestia` adapter to support multiple namespaces. This changes are
  transparent to the `RollupBlueprint`.

- #361 starts charging gas for submitting transactions to the Rollup. When calling `apply_slot`, the transaction sender
  must pay for a fixed amount of gas - `GAS_TX_FIXED_COST`. Developers have to make sure the transaction sender has
  enough funds to pay for the gas.

- #385 makes the `reward_burn_rate` constant in the `ProverIncentives` module and transforms the associated getters to
  be infallible. In the future, the reward burn rate will have to be set in the `constants.toml` and
  the `constants.testing.toml` files and need to be a value in the range [0, 99].

- #340 moves the Kernels' implementation (currently the `BasicKernel` and the `SoftConfirmationsKernel`) to a
  dedicated `sov-kernel` crate.

- #347 renames the following types:
  `StateTransitionData` to `StateTransitionWitness`
  `StateTransition` to `StateTransitionPublicData`
  `AggregatedProofPublicInput` to `AggregatedProofPublicData`

- #329 adds `InnerZkvm` and `OuterZkvm` associated types to the `Spec` trait.

- #306 removes the `State*Accessor` traits and replaces them with methods on (Acessory)StateValue/Map types. You can
  simply remove
  any imports of these traits and the `sov_modules_api::prelude*`. Also simplifies the API of VersionedStateValue. Now
  it only has a method `get_current` (for any type implementing the `VersionReader` trait)
  and get/set implemented directly on `KernelWorkingSet`

- #266 implements reward/slashing mechanisms for provers in the `ProverIncentives` module. In particular, given that an
  aggregated proof can be correctly serialized and the proof outputs are corrects, the provers can be rewarded for the
  new block transitions they proved. If no new block transitions are proved as part of the aggregated proof, then the
  prover is penalized by a fixed amount.
  The prover may be slashed if it posts an invalid proof or a proof for a state transition that doesn't exist.

- #170 unifies `CacheKey/Value` and `StorageKey/Value` data structures into `SlotKey/Value` data structures.

- #253 adds block validity conditions as part of aggregated proofs public inputs. This then can ensure that the validity
  conditions are stored on-chain for out-of-circuit verification. The validity conditions are stored as a `Vec<u8>`,
  after being serialized using `Borsh`.

- #242 changes the behavior of the `AttesterIncentives` module to gracefully exit when users are slashed and the state
  gets updated. The slashing reason can be retrieved as part of the `UserSlashed` event that gets emitted. Also contains
  small changes to the traits derived by the structures contained in the module, so that the module can be included in
  the runtime structures. We also add the `Checker` associated type to the `DaSpec` trait which considerably simplifies
  the module structure definition (contains two generics instead of 4)

- #169 achieves the rollup state separation in different namespaces. Conceptually, each namespace is just defined by a
  triple of tables inside a shared state db - there is only one `StateDb`.
- #451 Removes optional transactions list from RPC endpoints `eth_publishBatch`.
