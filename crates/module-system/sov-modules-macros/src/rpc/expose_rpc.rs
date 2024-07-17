use proc_macro::TokenStream;
use quote::quote;
use syn::DeriveInput;

use crate::common::StructFieldExtractor;

pub fn expose_rpc(
    proc_macro_name: &'static str,
    original: TokenStream,
    input: DeriveInput,
) -> syn::Result<TokenStream> {
    let field_extractor = StructFieldExtractor::new(proc_macro_name);

    let DeriveInput {
        data,
        generics,
        ident: input_ident,
        ..
    } = input;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let merge_operations = field_extractor
        .get_fields_from_struct(&data)?
        .iter()
        .map(|field| {
            let attrs = &field.attrs;
            let module_ident = &field.ident;

            quote! {
                #(#attrs)*
                rpc_module
                    .merge((&runtime.#module_ident).rpc_methods(storage.clone()))
                    .unwrap();
            }
        })
        .collect::<Vec<_>>();

    let runtime_type = quote! {
        #input_ident #ty_generics
    };

    let fn_get_rpc_methods: proc_macro2::TokenStream = quote! {
        /// Returns a [`jsonrpsee::RpcModule`] with all the RPC methods
        /// exposed by the runtime.
        pub fn get_rpc_methods #impl_generics (storage: ::jsonrpsee::tokio::sync::watch::Receiver<
            S::Storage
        >) -> ::jsonrpsee::RpcModule<()> #where_clause {
            use sov_modules_api::__rpc_macros_private::ModuleWithRpcServer;

            let runtime = <#runtime_type as ::core::default::Default>::default();
            let mut rpc_module = ::jsonrpsee::RpcModule::new(());

            #(#merge_operations)*
            rpc_module
        }
    };

    let mut tokens = TokenStream::new();

    tokens.extend(original);
    tokens.extend(TokenStream::from(fn_get_rpc_methods));

    Ok(tokens)
}
