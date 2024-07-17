//! Procedural macros to assist in the creation of Sovereign SDK modules and rollups.
//!
//! # Are you lost?
//!
//! This crate is **not** intended to be used directly!
//!
//! **Please refer to the documentation of `sov-modules-api` for
//! more information.**
//!
//! # Notes about authoring proc-macros
//!
//! This section serves as a collection of resources and useful tips for
//! authoring new proc-macros and maintaining the proc-macros present in this
//! crate.
//!
//! <div class="warning">
//! This place is a message...and part of a
//! system of messages...pay attention to it!
//!
//! Sending this message was important to us.
//! We considered ourselves to be a powerful culture.
//!
//! This place is not a place of honor... no
//! highly esteemed deed is commemorated here.
//!
//! What is here was dangerous and repulsive to us.
//! This message is a warning about danger.[^nuclear-warning]
//! </div>
//!
//! [^nuclear-warning]: <https://en.wikipedia.org/wiki/Long-term_nuclear_waste_warning_messages#Message>
//!
//! ## The `#[automatically_derived]` attribute
//!
//! Whenever your proc-macro generates a trait implementation, you should mark
//! it as `#[automatically_derived]`. It's good practice, to help the compiler
//! silence some warnings originating from generated code.
//!
//! See <https://stackoverflow.com/questions/51481551/what-does-automatically-derived-mean>.
//!
//! ## Separate parsing and generation
//!
//! It's almost always a good idea to cleanly separate your proc-macro parsing
//! logic from the code generation logic. Your proc-macro should ideally be composed of:
//!
//! 1. A parsing function, which consumes tokens and returns an instante of some
//!    custom type which provides well-typed informations about the input.
//!    [`darling`] is excellent for this.
//! 2. A code generator, which consumes the well-typed, parsed input and
//!    produces tokens.
//!
//! Besides readability, an extra benefit of this approach is that different
//! proc-macros can invoke each other's parsing logic if needed, and compose
//! nicely.
//!
//! ## Inner vs outer feature gating
//!
//! Sometimes your proc-macro is only intended to be used when a specific Cargo
//! feature is enabled, e.g. `native`. You usually have two choices:
//!
//! 1. Have the use feature-gate the proc-macro use itself, like this:
//!
//!    ```
//!    #[cfg_attr(feature = "native", derive(Clone))]
//!    struct MyStruct;
//!    ```
//!
//! 2. Modify the proc-macro to generate feature-gated code, resulting in more
//!    typical derive's, like this:
//!
//!    ```
//!    #[derive(Clone)]
//!    struct MyStruct;
//!    ```
//!
//! Option (1) is more verbose, whereas option (2) is more concise but requires
//! hard-coding the feature name.
//!
//! ## The `const _: () = {};` trick
//!
//! The `const _: () = {};` trick is a simple, yet effective way of creating a
//! new scope for you to generate code into, without worrying about
//! polluting the parent module.
//!
//! The key factor to consider here is that you won't be able to re-export
//! anything defined inside the scope that you define this way, so this trick is
//! appropriate when your proc-macro is e.g. implementing a trait, and not when
//! it's defining e.g. a new `struct` definition.
//!
//! Here's an example:
//!
//! ```
//! pub struct Foo(u32);
//!
//! const _: () = {
//!     use std::marker::PhantomData;
//!     use std::string::ToString;
//!
//!     #[automatically_derived]
//!     impl ToString for Foo {
//!         fn to_string(&self) -> String {
//!             format!("{}", self.0)
//!         }
//!     }
//! };
//! ```
//!
//! This trick is used by `serde` and other popular crates:
//! <https://github.com/serde-rs/serde/blob/3202a6858a2802b5aba2fa5cf3ec8f203408db74/serde_derive/src/dummy.rs#L15-L22>.
//!
//! ## Helper crates
//!
//! There's some incredible crates out there that can help tons when writing
//! even moderately complex proc-macros. If you're not familiar with the
//! proc-macro ecosystem, take some time to go through these lists and read the
//! descriptions of major crates to see if any of them can make your life
//! easier:
//!
//! - <https://lib.rs/development-tools/procedural-macro-helpers?sort=popular>
//! - <https://github.com/dtolnay/proc-macro-workshop>
//! - <https://old.reddit.com/r/rust/comments/16kdb3a/best_ways_to_learn_how_to_build_procedural_macros/>

// This crate is `missing_docs` because it is not intended to be used directly,
// but only through the re-exports in `sov_modules_api`. All re-exports are
// documented there.
#![allow(missing_docs)]

#[cfg(feature = "native")]
mod cli_parser;
mod common;
mod compile_manifest_constants;
mod dispatch;
mod event;
mod expand_macro;
mod manifest;
mod module_info;
mod new_types;
mod offchain;
mod rest;
#[cfg(feature = "native")]
mod rpc;

use compile_manifest_constants::{make_const_bech32, make_const_value};
use dispatch::dispatch_call::DispatchCallMacro;
use dispatch::genesis::GenesisMacro;
use dispatch::message_codec::MessageCodec;
use event::EventMacro;
use module_info::ModuleType;
use new_types::address_type_helper;
use offchain::offchain_generator;
use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput, ItemFn};

/// Returns the name of the function that invoked the proc-macro.
// Shamelessly copy-pasted from <https://stackoverflow.com/a/40234666/5148606>.
macro_rules! fn_name {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        // We wouldn't want to crash if something goes wrong here (that would be
        // very confusing!).
        name.strip_suffix("::f")
            .unwrap_or("UNKNOWN")
            .split("::")
            .last()
            .unwrap_or("UNKNOWN")
    }};
}

#[proc_macro_derive(ModuleInfo, attributes(state, module, kernel_module, id, gas, phantom))]
pub fn module_info(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);

    handle_macro_error_and_expand(
        fn_name!(),
        module_info::derive_module_info(input, ModuleType::Standard),
    )
}

#[proc_macro_derive(
    KernelModuleInfo,
    attributes(state, module, kernel_module, id, gas, phantom)
)]
pub fn kernel_module_info(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);

    handle_macro_error_and_expand(
        fn_name!(),
        module_info::derive_module_info(input, ModuleType::Kernel),
    )
}

#[proc_macro_derive(Genesis)]
pub fn genesis(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    let genesis_macro = GenesisMacro::new("Genesis");

    handle_macro_error_and_expand(fn_name!(), genesis_macro.derive_genesis(input))
}

#[proc_macro_derive(DispatchCall, attributes(serialization))]
pub fn dispatch_call(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    let call_macro = DispatchCallMacro::new("Call");

    handle_macro_error_and_expand(fn_name!(), call_macro.derive_dispatch_call(input))
}

#[proc_macro_derive(Event, attributes(serialization))]
pub fn event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    let event_macro = EventMacro::new("Event");

    handle_macro_error_and_expand(fn_name!(), event_macro.derive_event_enum(input))
}

/// Adds encoding functionality to the underlying type.
#[proc_macro_derive(MessageCodec)]
pub fn codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    let codec_macro = MessageCodec::new("MessageCodec");

    handle_macro_error_and_expand(fn_name!(), codec_macro.derive_message_codec(input))
}

#[proc_macro]
pub fn config_bech32(tokens: TokenStream) -> TokenStream {
    struct ConstBech32Input {
        lit_str: syn::LitStr,
        ty: syn::Type,
    }

    impl syn::parse::Parse for ConstBech32Input {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            let lit_str = input.parse()?;
            input.parse::<syn::Token![,]>()?;
            let ty = input.parse()?;
            Ok(ConstBech32Input { lit_str, ty })
        }
    }

    let ConstBech32Input { lit_str, ty } = parse_macro_input!(tokens as ConstBech32Input);
    handle_macro_error_and_expand(fn_name!(), make_const_bech32(&lit_str, &ty))
}

#[proc_macro]
pub fn config_value(item: TokenStream) -> TokenStream {
    let constant_name = parse_macro_input!(item as syn::LitStr);
    handle_macro_error_and_expand(fn_name!(), make_const_value(&constant_name).map(Into::into))
}

#[proc_macro_attribute]
#[cfg(feature = "native")]
pub fn rpc_gen(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_contents: Vec<syn::NestedMeta> = parse_macro_input!(attr);
    let input = parse_macro_input!(item as syn::ItemImpl);
    handle_macro_error_and_expand(
        fn_name!(),
        rpc::rpc_gen(attr_contents, input).map(Into::into),
    )
}

#[cfg(feature = "native")]
#[proc_macro_attribute]
pub fn expose_rpc(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let original = input.clone();
    let input = parse_macro_input!(input);
    handle_macro_error_and_expand(fn_name!(), rpc::expose_rpc("Expose", original, input))
}

#[proc_macro_derive(RuntimeRestApi, attributes(rest_api))]
pub fn runtime_metadata_rest_api(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    handle_macro_error_and_expand(fn_name!(), rest::runtime::derive(input).map(Into::into))
}

#[proc_macro_derive(ModuleRestApi, attributes(rest_api))]
pub fn module_metadata_rest_api(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    handle_macro_error_and_expand(fn_name!(), rest::module::derive(input).map(Into::into))
}

#[cfg(feature = "native")]
#[proc_macro_derive(CliWallet, attributes(cli_skip))]
pub fn cli_parser(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input);
    handle_macro_error_and_expand(fn_name!(), cli_parser::derive_cli_wallet("Cmd", input))
}

#[cfg(feature = "native")]
#[proc_macro_derive(CliWalletArg)]
pub fn custom_enum_clap(input: TokenStream) -> TokenStream {
    let input: syn::DeriveInput = parse_macro_input!(input);
    handle_macro_error_and_expand(fn_name!(), cli_parser::derive_cli_wallet_arg(input))
}

#[proc_macro_attribute]
pub fn address_type(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    handle_macro_error_and_expand(fn_name!(), address_type_helper(input))
}

#[proc_macro_attribute]
pub fn offchain(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    handle_macro_error_and_expand(fn_name!(), offchain_generator(input))
}

fn expand_code(macro_name: &str, input: TokenStream) -> TokenStream {
    if std::env::var_os("SOVEREIGN_SDK_EXPAND_PROC_MACROS").is_some() {
        expand_macro::expand_to_file(input.clone(), macro_name).unwrap_or_else(|err| {
            eprintln!(
                "Failed to write to file proc-macro generated code: {:?}",
                err
            );
            input
        })
    } else {
        input
    }
}

fn handle_macro_error_and_expand(
    macro_name: &str,
    result: Result<proc_macro::TokenStream, syn::Error>,
) -> TokenStream {
    expand_code(
        macro_name,
        result.unwrap_or_else(|err| err.to_compile_error().into()),
    )
}
