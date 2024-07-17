/// Derives the [`DispatchCall`] trait for the underlying
/// type.
pub use sov_modules_macros::DispatchCall;
/// Derives the <runtime_name>Event enum for a given runtime.
pub use sov_modules_macros::Event;
/// Derives the [`Genesis`](trait.Genesis.html) trait for the underlying runtime
/// `struct`.
pub use sov_modules_macros::Genesis;
/// Derives the [`ModuleInfo`] trait for the underlying `struct`, giving full access to kernel functionality
pub use sov_modules_macros::KernelModuleInfo;
pub use sov_modules_macros::MessageCodec;
/// Derives the [`ModuleInfo`] trait for the underlying `struct`.
///
/// The underlying type must respect the following conditions, or compilation
/// will fail:
/// - It must be a named `struct`. Tuple `struct`s, `enum`s, and others are
/// not supported.
/// - It must have *exactly one* field with the `#[id]` attribute. This field
///   represents the **module id**.
/// - All other fields must have either the `#[state]` or `#[module]` attribute.
///   - `#[state]` is used for state members.
///   - `#[module]` is used for module members.
///
/// In addition to implementing [`ModuleInfo`], this macro will
/// also generate so-called "prefix" methods.
///
/// ## Example
///
/// ```
/// use sov_modules_api::{Spec, ModuleId, ModuleInfo, StateMap};
///
/// #[derive(ModuleInfo)]
/// struct TestModule<S: Spec> {
///     #[id]
///     id: ModuleId,
///
///     #[state]
///     pub state_map: StateMap<String, u32>,
///
///     #[phantom]
///     phantom: std::marker::PhantomData<S>,
/// }
///
/// // You can then get the prefix of `state_map` like this:
/// fn get_prefix<S: Spec>(some_storage: S::Storage) {
///     let test_struct = TestModule::<S>::default();
///     let prefix1 = test_struct.state_map.prefix();
/// }
/// ```
pub use sov_modules_macros::ModuleInfo;

/// Procedural macros to assist with creating new modules.
pub mod macros {
    /// Simple convenience macro for adding some common derive macros and
    /// impls specifically for a NewType wrapping an Address.
    /// The reason for having this is that we assumes NewTypes for address as a common use case
    ///
    /// ## Example
    /// ```
    /// use sov_modules_macros::address_type;
    /// use std::fmt;
    /// use sov_modules_api::Spec;
    /// #[address_type]
    /// pub struct UserAddress;
    /// ```
    ///
    /// This is exactly equivalent to hand-writing
    ///
    /// ```
    /// use std::fmt;
    /// use sov_modules_api::Spec;
    /// #[cfg(feature = "native")]
    /// #[derive(schemars::JsonSchema)]
    /// #[schemars(bound = "S::Address: ::schemars::JsonSchema", rename = "UserAddress")]
    /// #[derive(borsh::BorshDeserialize, borsh::BorshSerialize, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
    /// pub struct UserAddress<S: Spec>(S::Address);
    ///
    /// #[cfg(not(feature = "native"))]
    /// #[derive(borsh::BorshDeserialize, borsh::BorshSerialize, serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
    /// pub struct UserAddress<S: Spec>(S::Address);
    ///
    /// impl<S: Spec> UserAddress<S> {
    ///     /// Public constructor
    ///     pub fn new(address: &S::Address) -> Self {
    ///         UserAddress(address.clone())
    ///     }
    ///
    ///     /// Public getter
    ///     pub fn get_address(&self) -> &S::Address {
    ///         &self.0
    ///     }
    /// }
    ///
    /// impl<S: Spec> fmt::Display for UserAddress<S>
    /// where
    ///     S::Address: fmt::Display,
    /// {
    ///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    ///         write!(f, "{}", self.0)
    ///     }
    /// }
    ///
    /// impl<S: Spec> AsRef<[u8]> for UserAddress<S>
    /// where
    ///     S::Address: AsRef<[u8]>,
    /// {
    ///     fn as_ref(&self) -> &[u8] {
    ///         self.0.as_ref()
    ///     }
    /// }
    /// ```
    pub use sov_modules_macros::address_type;
    /// Reads a string value from the rollup configuration manifest file and
    /// decodes it as a Bech32 value.
    ///
    /// The macro takes two arguments:
    ///  1. The name of the constant to be read from the manifest file, as a string literal.
    ///  2. The Bech32 newtype to decode the value into. This type must be
    ///     defined by [`impl_hash32_type`](crate::impl_hash32_type).
    pub use sov_modules_macros::config_bech32;
    /// Reads a TOML value from the rollup configuration manifest file and
    /// converts it into a Rust expression available at compile time. Nulls and
    /// objects are not supported.
    pub use sov_modules_macros::config_value;
    /// The macro exposes RPC endpoints from all modules in the runtime.
    /// It gets storage from the Context generic
    /// and utilizes output of [`#[rpc_gen]`] macro to generate RPC methods.
    ///
    /// It has limitations:
    ///   - First type generic attribute must have bound to [`Context`](crate::Context) trait
    ///   - All generic attributes must own the data, thus have bound `'static`
    #[cfg(feature = "native")]
    pub use sov_modules_macros::expose_rpc;
    /// The offchain macro is used to annotate functions that should only be executed by the rollup
    /// when the "offchain" feature flag is passed. The macro produces one of two functions depending on
    /// the presence flag.
    /// "offchain" feature enabled: function is present as defined
    /// "offchain" feature absent: function body is replaced with an empty definition
    ///
    /// The idea here is that offchain computation is optionally enabled for a full node and is not
    /// part of chain state and does not impact consensus, prover or anything else.
    ///
    /// ## Example
    /// ```
    /// use sov_modules_macros::offchain;
    /// #[offchain]
    /// fn redis_insert(count: u64){
    ///     println!("Inserting {} to redis", count);
    /// }
    /// ```
    ///
    /// This is exactly equivalent to hand-writing
    /// ```
    /// #[cfg(feature = "offchain")]
    /// fn redis_insert(count: u64){
    ///     println!("Inserting {} to redis", count);
    /// }
    ///
    /// #[cfg(not(feature = "offchain"))]
    /// fn redis_insert(count: u64){
    /// }
    /// ```
    pub use sov_modules_macros::offchain;
    /// A wrapper around [`jsonrpsee::proc_macros::rpc`] for modules.
    ///
    /// This proc-macro generates a [`jsonrpsee`] implementation for the underlying
    /// module type. It behaves very similar to the original [`jsonrpsee`]
    /// proc-macro, but with some important distinctions:
    ///
    /// 1. It's called `#[rpc_gen]` instead of `#[rpc]`, to avoid confusion with the
    ///    original proc-macro.
    /// 2. `#[method]` is renamed to with `#[rpc_method]` to avoid confusion with
    ///    [`jsonrpsee`]'s own attribute.
    /// 3. **It's applied on an `impl` block instead of a trait.** [`macro@rpc_gen`] will
    ///    copy all method definitions from your `impl` block into a new trait with
    ///    the same generics and method signatures. Unlike [`jsonrpsee`] traits
    ///    which can simply be signatures, these methods must all have function
    ///    bodies as they provide the trait definition and its implementation at the
    ///    same time.
    /// 4. Working set arguments with signature `state: &mut WorkingSet<S>`
    ///    are automatically removed from the method signatures (as they are not
    ///    [`serde`]-compatible) and injected directly within the method bodies.
    ///
    ///    It sounds more complicated than it is. Generally, you can just assume
    ///    that the proc-macro will provide you with a working set argument that you
    ///    can request by adding it as a method argument.
    ///
    /// Any code relying on this macro must take [`jsonrpsee`] as a dependency with
    /// at least the following features enabled:
    ///
    /// ```toml
    /// jsonrpsee = { version = "...", features = ["macros", "client-core", "server"] }
    /// ```
    ///
    /// This proc-macro is only intended for modules. Refer to [`macro@expose_rpc`] for
    /// the runtime proc-macro.
    ///
    /// ## Example
    /// ```
    /// use sov_modules_api::{Spec, StateValue, ModuleId, ModuleInfo, ApiStateAccessor, prelude::UnwrapInfallible};
    /// use sov_modules_api::macros::rpc_gen;
    /// use jsonrpsee::core::RpcResult;
    ///
    /// #[derive(ModuleInfo)]
    /// struct MyModule<S: Spec> {
    ///     #[id]
    ///     id: ModuleId,
    ///     #[state]
    ///     values: StateValue<S::Address>,
    ///     // ...
    /// }
    ///
    /// #[rpc_gen(client, server, namespace = "myNamespace")]
    /// impl<S: Spec> MyModule<S> {
    ///     #[rpc_method(name = "myMethod")]
    ///     fn my_method(&self, state: &mut ApiStateAccessor<S>, param: u32) -> RpcResult<S::Address> {
    ///         Ok(self.values.get(state).unwrap_infallible().unwrap())
    ///     }
    /// }
    /// ```
    #[cfg(feature = "native")]
    pub use sov_modules_macros::rpc_gen;
    /// Implements the `sov_modules_api::CliWallet` trait for the annotated runtime.
    /// Under the hood, this macro generates an enum called `CliTransactionParser` which derives the [`clap::Parser`] trait.
    /// This enum has one variant for each field of the `Runtime`, and uses the `sov_modules_api::CliWalletArg` trait to parse the
    /// arguments for each of these structs.
    ///
    /// To exclude a module from the CLI, use the `#[cli_skip]` attribute.
    ///
    /// ## Examples
    /// ```
    /// use sov_modules_api::{Spec, DispatchCall, MessageCodec};
    /// use sov_modules_api::macros::CliWallet;
    ///
    /// #[derive(DispatchCall, MessageCodec, CliWallet)]
    /// #[serialization(borsh::BorshDeserialize, borsh::BorshSerialize)]
    /// pub struct Runtime<S: Spec> {
    ///     pub bank: sov_bank::Bank<S>,
    ///     // ...
    /// }
    ///
    /// fn main() {}
    //  ^^^^^^^^^^^^
    //  COMMENT: the above `main` function is a workaround for
    //  <https://github.com/rust-lang/rust/issues/83583#issuecomment-1083300448>.
    /// ```
    #[cfg(feature = "native")]
    pub use sov_modules_macros::CliWallet;
    /// Implement [`CliWalletArg`](crate::CliWalletArg) for the annotated struct or enum. Unions are not supported.
    ///
    /// Under the hood, this macro generates a new struct or enum which derives the [`clap::Parser`] trait, and then implements the
    /// [`CliWalletArg`](crate::CliWalletArg) trait where the `CliStringRepr` type is the new struct or enum.
    ///
    /// As an implementation detail, `clap` requires that all types have named fields - so this macro auto generates an appropriate
    /// `clap`-compatible type from the annotated item. For example, the struct `MyStruct(u64, u64)` would be transformed into
    /// `MyStructWithNamedFields { field0: u64, field1: u64 }`.
    ///
    /// ## Example
    ///
    /// This code..
    /// ```rust
    /// use sov_modules_api::macros::CliWalletArg;
    ///
    /// #[derive(CliWalletArg, Clone)]
    /// pub enum MyEnum {
    ///    /// A number
    ///    Number(u32),
    ///    /// A hash
    ///    Hash { hash: String },
    /// }
    /// ```
    ///
    /// ...expands into the following code:
    /// ```rust,ignore
    /// // The original enum definition is left in its original place
    /// pub enum MyEnum {
    ///    /// A number
    ///    Number(u32),
    ///    /// A hash
    ///    Hash { hash: String },
    /// }
    ///
    /// // We generate a new enum with named fields which can derive `clap::Parser`.
    /// // Since this variant is only ever converted back to the original, we
    /// // don't carry over any of the original derives. However, we do preserve
    /// // doc comments from the original version so that `clap` can display them.
    /// #[derive(::clap::Parser)]
    /// pub enum MyEnumWithNamedFields {
    ///    /// A number
    ///    Number { field0: u32 } ,
    ///    /// A hash
    ///    Hash { hash: String },
    /// }
    /// // We generate a `From` impl to convert between the types.
    /// impl From<MyEnumWithNamedFields> for MyEnum {
    ///    fn from(item: MyEnumWithNamedFields) -> Self {
    ///       match item {
    ///         Number { field0 } => MyEnum::Number(field0),
    ///         Hash { hash } => MyEnum::Hash { hash },
    ///       }
    ///    }
    /// }
    ///
    /// impl sov_modules_api::CliWalletArg for MyEnum {
    ///     type CliStringRepr = MyEnumWithNamedFields;
    /// }
    /// ```
    #[cfg(feature = "native")]
    pub use sov_modules_macros::CliWalletArg;
    /// Derives [`HasRestApi`](crate::rest::HasRestApi) for modules.
    ///
    /// REST APIs generated with this proc-macro will serve static metadata
    /// about the module itself, such as:
    /// - its name;
    /// - its description;
    /// - its [`ModuleId`](crate::ModuleId).
    ///
    /// In addition to static metadata, the API also provides access to
    /// the module's state items (e.g. [`StateMap`](crate::containers::StateMap))'s
    /// values, both at the latest block and at specific block heights.
    /// The root path contains a listing of all state items that can be queried
    /// through the API.
    ///
    /// ## Attributes: `#[rest_api(skip)]`
    ///
    /// Tells the proc-macro to **NOT** provide access to a specific state item
    /// within the module.
    ///
    /// ```
    /// use sov_modules_api::prelude::*;
    /// use sov_modules_api::{ModuleId, ModuleInfo, StateValue};
    ///
    /// #[derive(Clone, ModuleInfo, ModuleRestApi)]
    /// struct MyModule<S: Spec> {
    ///     #[id]
    ///     id: ModuleId,
    ///     /// This state item can't be queried through the API.
    ///     #[state]
    ///     #[rest_api(skip)]
    ///     state_item: StateValue<S::Address>,
    /// }
    /// # // BEGIN MODULE IMPL, COPY-PASTE-ME (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#hiding-portions-of-the-example)
    /// # impl<S: Spec> sov_modules_api::Module for MyModule<S> {
    /// #    type Spec = S;
    /// #    type Config = ();
    /// #    type CallMessage = ();
    /// #    type Event = ();
    /// #
    /// #    fn genesis(
    /// #        &self,
    /// #        _config: &Self::Config,
    /// #        _state: &mut impl sov_modules_api::state::GenesisState<S>,
    /// #    ) -> Result<(), sov_modules_api::Error> {
    /// #        Ok(())
    /// #    }
    /// #
    /// #    fn call(
    /// #        &self,
    /// #        _msg: Self::CallMessage,
    /// #        _context: &Context<Self::Spec>,
    /// #        _state: &mut impl sov_modules_api::state::TxState<S>,
    /// #    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
    /// #        unimplemented!()
    /// #    }
    /// # }
    /// # // END MODULE IMPL
    /// ```
    ///
    /// ## Attributes: `#[rest_api(include)]`
    ///
    /// Tells the proc-macro that compilation **MUST** fail if the marked state
    /// item can't be exposed through the API, e.g. for unsatisfied trait
    /// bounds, instead of silently ignoring the item.
    ///
    /// ```
    /// use sov_modules_api::prelude::*;
    /// use sov_modules_api::{ModuleId, ModuleInfo, StateValue};
    ///
    /// #[derive(Clone, ModuleInfo, ModuleRestApi)]
    /// struct MyModule<S: Spec> {
    ///     #[id]
    ///     id: ModuleId,
    ///     /// If someone were to replace `S::Address` with a type that doesn't
    ///     /// satisfy the necessary trait bounds, the compiler will complain.
    ///     #[state]
    ///     #[rest_api(include)]
    ///     state_item: StateValue<S::Address>,
    /// }
    /// # // BEGIN MODULE IMPL, COPY-PASTE-ME (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#hiding-portions-of-the-example)
    /// # impl<S: Spec> sov_modules_api::Module for MyModule<S> {
    /// #    type Spec = S;
    /// #    type Config = ();
    /// #    type CallMessage = ();
    /// #    type Event = ();
    /// #
    /// #    fn genesis(
    /// #        &self,
    /// #        _config: &Self::Config,
    /// #        _state: &mut impl sov_modules_api::state::GenesisState<S>,
    /// #    ) -> Result<(), sov_modules_api::Error> {
    /// #        Ok(())
    /// #    }
    /// #
    /// #    fn call(
    /// #        &self,
    /// #        _msg: Self::CallMessage,
    /// #        _context: &Context<Self::Spec>,
    /// #        _state: &mut impl sov_modules_api::state::TxState<S>,
    /// #    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
    /// #        unimplemented!()
    /// #    }
    /// # }
    /// # // END MODULE IMPL
    /// ```
    ///
    /// ## Attributes: `#[rest_api(doc = "...")]`
    ///
    /// Overrides the description of the marked item used in the generated
    /// metadata. By default, descriptions are fetched from docstrings.
    ///
    /// You can use this attribute at the top of the module as well as state items.
    ///
    /// ```
    /// use sov_modules_api::prelude::*;
    /// use sov_modules_api::{ModuleId, ModuleInfo, StateMap};
    ///
    /// /// This docstring will not be used.
    /// #[derive(Clone, ModuleInfo, ModuleRestApi)]
    /// #[rest_api(doc = "This is a description of the module.")]
    /// #[rest_api(doc = "")]
    /// #[rest_api(doc = "This is a second paragraph in the description.")]
    /// struct MyModule<S: Spec> {
    ///     #[id]
    ///     id: ModuleId,
    ///     /// This description will not be used.
    ///     #[state]
    ///     #[rest_api(doc = "My favorite state item!")]
    ///     state_item: StateMap<S::Address, u64>,
    /// }
    /// # // BEGIN MODULE IMPL, COPY-PASTE-ME (https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html#hiding-portions-of-the-example)
    /// # impl<S: Spec> sov_modules_api::Module for MyModule<S> {
    /// #    type Spec = S;
    /// #    type Config = ();
    /// #    type CallMessage = ();
    /// #    type Event = ();
    /// #
    /// #    fn genesis(
    /// #        &self,
    /// #        _config: &Self::Config,
    /// #        _state: &mut impl sov_modules_api::state::GenesisState<S>,
    /// #    ) -> Result<(), sov_modules_api::Error> {
    /// #        Ok(())
    /// #    }
    /// #
    /// #    fn call(
    /// #        &self,
    /// #        _msg: Self::CallMessage,
    /// #        _context: &Context<Self::Spec>,
    /// #        _state: &mut impl sov_modules_api::state::TxState<S>,
    /// #    ) -> Result<sov_modules_api::CallResponse, sov_modules_api::Error> {
    /// #        unimplemented!()
    /// #    }
    /// # }
    /// # // END MODULE IMPL
    /// ```
    pub use sov_modules_macros::ModuleRestApi;
    /// Derives [`HasRestApi`](crate::rest::HasRestApi) for runtimes.
    ///
    /// For each module listed in this runtime, the proc-macro will mount its
    /// own REST API at
    /// `/modules/<hyphenated-module-name>`. Consult the documentation of
    /// [`crate::rest`] for more information about these traits.
    ///
    /// Modules listed in the runtime for which no
    /// [`ModuleRestApi`] is derived will simply be ignored.
    ///
    /// ## Attributes: `#[rest_api(skip)]`
    ///
    /// Tells the proc-macro to **NOT** generate a REST API for the marked module.
    ///
    /// ## Attributes: `#[rest_api(doc)]`
    ///
    /// This attribute behaves exactly the same as it does for
    /// [`ModuleRestApi`].
    pub use sov_modules_macros::RuntimeRestApi;
}
