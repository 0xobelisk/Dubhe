use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

pub fn offchain_generator(function: ItemFn) -> syn::Result<TokenStream> {
    let ItemFn {
        vis, sig, block, ..
    } = function;

    Ok(quote! {
        // The "real" function
        #[cfg(feature = "offchain")]
        #vis #sig {
            #block
        }

        // The no-op function
        #[cfg(not(feature = "offchain"))]
        #[allow(unused_variables)]
        #vis #sig {
            // Do nothing. Will be optimized away.
        }
    }
    .into())
}
