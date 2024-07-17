use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, ToTokens};
use syn::spanned::Spanned;
use syn::{
    DataStruct, Fields, GenericParam, ImplGenerics, Meta, TypeGenerics, VisPublic, Visibility,
};

#[derive(Clone)]
pub(crate) struct StructNamedField {
    pub(crate) ident: proc_macro2::Ident,
    pub(crate) ty: syn::Type,
    pub(crate) attrs: Vec<syn::Attribute>,
    pub(crate) vis: syn::Visibility,
}

impl StructNamedField {
    #[cfg_attr(not(feature = "native"), allow(unused))]
    pub(crate) fn filter_attrs(&mut self, filter: impl FnMut(&syn::Attribute) -> bool) {
        self.attrs = std::mem::take(&mut self.attrs)
            .into_iter()
            .filter(filter)
            .collect();
    }

    #[cfg_attr(not(feature = "native"), allow(unused))]
    pub(crate) fn contains_attr(&self, attr_ident: &str) -> bool {
        self.attrs.iter().any(|attr| attr.path.is_ident(attr_ident))
    }
}

impl ToTokens for StructNamedField {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let docs = &self.attrs;
        let vis = &self.vis;
        let ident = &self.ident;
        let ty = &self.ty;
        tokens.extend(quote::quote! {
            #( #docs )*
            #vis #ident: #ty
        });
    }
}

pub(crate) struct StructFieldExtractor {
    macro_name: &'static str,
}

impl StructFieldExtractor {
    pub(crate) fn new(macro_name: &'static str) -> Self {
        Self { macro_name }
    }

    // Extracts named fields form a struct or emits an error.
    pub(crate) fn get_fields_from_struct(
        &self,
        data: &syn::Data,
    ) -> Result<Vec<StructNamedField>, syn::Error> {
        match data {
            syn::Data::Struct(data_struct) => self.get_fields_from_data_struct(data_struct),
            syn::Data::Enum(en) => Err(syn::Error::new_spanned(
                en.enum_token,
                format!("The {} macro supports structs only.", self.macro_name),
            )),
            syn::Data::Union(un) => Err(syn::Error::new_spanned(
                un.union_token,
                format!("The {} macro supports structs only.", self.macro_name),
            )),
        }
    }

    /// Extract the named fields from a struct, or generate named fields matching the fields of an unnamed struct.
    /// Names follow the pattern `field0`, `field1`, etc..
    ///
    /// The `public` parameter, if set, makes it so the generated fields are
    /// public; if not, the parent visibility is used.
    #[cfg_attr(not(feature = "native"), allow(unused))]
    pub(crate) fn get_or_generate_named_fields(
        fields: &Fields,
        public: bool,
    ) -> Vec<StructNamedField> {
        match fields {
            Fields::Unnamed(unnamed_fields) => unnamed_fields
                .unnamed
                .iter()
                .enumerate()
                .map(|(i, field)| {
                    let ident = Ident::new(&format!("field{}", i), field.span());
                    let ty = &field.ty;
                    let vis = if public {
                        Visibility::Public(VisPublic {
                            pub_token: Default::default(),
                        })
                    } else {
                        field.vis.clone()
                    };
                    StructNamedField {
                        attrs: field.attrs.clone(),
                        vis,
                        ident,
                        ty: ty.clone(),
                    }
                })
                .collect::<Vec<_>>(),
            Fields::Named(fields_named) => fields_named
                .named
                .iter()
                .map(|field| {
                    let ty = &field.ty;
                    let vis = if public {
                        Visibility::Public(VisPublic {
                            pub_token: Default::default(),
                        })
                    } else {
                        field.vis.clone()
                    };
                    StructNamedField {
                        attrs: field.attrs.clone(),
                        vis,
                        ident: field.ident.clone().expect("Named fields must have names!"),
                        ty: ty.clone(),
                    }
                })
                .collect::<Vec<_>>(),
            Fields::Unit => Vec::new(),
        }
    }

    fn get_fields_from_data_struct(
        &self,
        data_struct: &DataStruct,
    ) -> Result<Vec<StructNamedField>, syn::Error> {
        let mut output_fields = Vec::default();

        for original_field in data_struct.fields.iter() {
            let field_ident = original_field
                .ident
                .as_ref()
                .ok_or(syn::Error::new_spanned(
                    &original_field.ident,
                    format!(
                        "The {} macro supports structs only, unnamed fields witnessed.",
                        self.macro_name
                    ),
                ))?;

            let field = StructNamedField {
                ident: field_ident.clone(),
                ty: original_field.ty.clone(),
                attrs: original_field.attrs.clone(),
                vis: original_field.vis.clone(),
            };

            output_fields.push(field);
        }
        Ok(output_fields)
    }
}

pub(crate) const CALL: &str = "Call";

/// Represents "parsed" rust struct.
pub(crate) struct StructDef<'a> {
    pub(crate) ident: proc_macro2::Ident,
    pub(crate) impl_generics: ImplGenerics<'a>,
    pub(crate) type_generics: TypeGenerics<'a>,
    pub(crate) generic_param: &'a Ident,
    pub(crate) fields: Vec<StructNamedField>,
    pub(crate) where_clause: Option<&'a syn::WhereClause>,
}

impl<'a> StructDef<'a> {
    pub(crate) fn new(
        ident: proc_macro2::Ident,
        fields: Vec<StructNamedField>,
        impl_generics: ImplGenerics<'a>,
        type_generics: TypeGenerics<'a>,
        generic_param: &'a Ident,
        where_clause: Option<&'a syn::WhereClause>,
    ) -> Self {
        Self {
            ident,
            fields,
            impl_generics,
            type_generics,
            generic_param,
            where_clause,
        }
    }

    /// Creates an enum type based on the underlying struct.
    pub(crate) fn create_enum(
        &self,
        enum_legs: &[proc_macro2::TokenStream],
        postfix: &'static str,
        serialization_attrs: &Vec<TokenStream>,
        extra_attributes: &[TokenStream],
    ) -> proc_macro2::TokenStream {
        let enum_ident = self.enum_ident(postfix);
        let impl_generics = &self.impl_generics;
        let where_clause = &self.where_clause;
        quote::quote! {
            #[allow(non_camel_case_types)]
            #[derive(::core::fmt::Debug, PartialEq, #(#serialization_attrs),*)]
            #(#extra_attributes)*
            pub enum #enum_ident #impl_generics #where_clause {
                #(#enum_legs)*
            }
        }
    }

    pub(crate) fn enum_ident(&self, postfix: &'static str) -> Ident {
        let ident = &self.ident;
        format_ident!("{ident}{postfix}")
    }
}

/// Gets the first type parameter's identifier from [`syn::Generics`].
pub(crate) fn get_generics_type_param(
    generics: &syn::Generics,
    error_span: Span,
) -> Result<Ident, syn::Error> {
    let generic_param = match generics
        .params
        .first()
        .ok_or_else(|| syn::Error::new(error_span, "No generic parameters found"))?
    {
        GenericParam::Type(ty) => &ty.ident,
        GenericParam::Lifetime(lf) => {
            return Err(syn::Error::new_spanned(
                lf,
                "Lifetime parameters are not supported.",
            ))
        }
        GenericParam::Const(cnst) => {
            return Err(syn::Error::new_spanned(
                cnst,
                "Const parameters are not supported.",
            ))
        }
    };

    Ok(generic_param.clone())
}

pub(crate) fn get_attribute_values(
    item: &syn::DeriveInput,
    attribute_name: &str,
) -> Vec<TokenStream> {
    let mut values = vec![];

    // Find the attribute with the given name on the root item
    item.attrs
        .iter()
        .filter(|attr| attr.path.is_ident(attribute_name))
        .for_each(|attr| {
            if let Ok(Meta::List(list)) = attr.parse_meta() {
                values.extend(list.nested.iter().map(|n| {
                    let mut tokens = TokenStream::new();
                    n.to_tokens(&mut tokens);
                    tokens
                }));
            }
        });

    values
}

fn syn_lit_to_expr(lit: syn::Lit) -> syn::Expr {
    syn::Expr::Lit(syn::ExprLit { attrs: vec![], lit })
}

/// Converts a TOML value into a Rust expression of the most appropriate type.
///
/// Nulls and objects are not supported because they don't map naturally to any
/// [`std`] Rust type.
pub fn toml_value_to_expr(value: &toml::Value, span: Span) -> syn::Result<syn::Expr> {
    use toml::Value;

    let error = |toml_type: &str| {
        syn::Error::new(span, format!("failed to convert TOML value into Rust expression; its TOML value type ({}) is not supported: `{:?}`", toml_type, value))
    };

    match value {
        Value::Table(_) => Err(error("table")),
        Value::Float(_) => Err(error("float")),
        Value::Datetime(_) => Err(error("datetime")),
        Value::Boolean(b) => Ok(syn_lit_to_expr(syn::Lit::Bool(syn::LitBool::new(*b, span)))),
        Value::Integer(num) => Ok(syn_lit_to_expr(syn::Lit::Int(syn::LitInt::new(
            &num.to_string(),
            span,
        )))),
        Value::String(s) => Ok(syn_lit_to_expr(syn::Lit::Str(syn::LitStr::new(s, span)))),
        Value::Array(arr) => {
            let values: Vec<syn::Expr> = arr
                .iter()
                .map(|v| toml_value_to_expr(v, span))
                .collect::<syn::Result<_>>()?;
            Ok(syn::Expr::Array(syn::ExprArray {
                attrs: Vec::new(),
                bracket_token: syn::token::Bracket { span },
                elems: syn::punctuated::Punctuated::from_iter(values),
            }))
        }
    }
}

pub(crate) fn get_serialization_attrs(
    item: &syn::DeriveInput,
) -> Result<Vec<TokenStream>, syn::Error> {
    const SERIALIZE: &str = "Serialize";
    const DESERIALIZE: &str = "Deserialize";

    let serialization_attrs = get_attribute_values(item, "serialization");

    let mut has_serialize = false;
    let mut has_deserialize = false;
    let mut has_other = false;

    let attributes: Vec<String> = serialization_attrs.iter().map(|t| t.to_string()).collect();

    for attr in &attributes {
        if attr.contains(SERIALIZE) {
            has_serialize = true;
        } else if attr.contains(DESERIALIZE) {
            has_deserialize = true;
        } else {
            has_other = true;
        }
    }

    let tokens: TokenStream = quote::quote! { serialization };
    if !has_serialize || !has_deserialize {
        return Err(syn::Error::new_spanned(
            &tokens,
            format!(
                "Serialization attributes must contain both '{}' and '{}', but contains '{:?}'",
                SERIALIZE, DESERIALIZE, &attributes
            ),
        ));
    } else if has_other {
        return Err(syn::Error::new_spanned(
            &tokens,
            format!("Serialization attributes can not contain attributes that are not '{}' and '{}', but contains: '{:?}'", 
                SERIALIZE, DESERIALIZE, &attributes.iter().filter(|a| !a.contains(SERIALIZE) && !a.contains(DESERIALIZE)).collect::<Vec<_>>()),
        ));
    }

    Ok(serialization_attrs)
}

// Converts a Rust identifier into a human-readable URL path segment on a
// "best-effort" basis (i.e. some identifiers may result in invalid or unreadable
// URLs).
pub fn str_to_url_segment(ident: &Ident) -> String {
    use convert_case::{Case, Casing};

    ident.to_string().to_case(Case::Kebab)
}

/// Wraps the code in a new scope using the `const _: () = {};` trick.
///
/// Adapted from MIT-licensed code here:
/// <https://github.com/serde-rs/serde/blob/3202a6858a2802b5aba2fa5cf3ec8f203408db74/serde_derive/src/dummy.rs#L15-L22>.
///
/// Copyright (c) David Tolnay and Serde contributors.
pub fn wrap_in_new_scope(code: TokenStream) -> TokenStream {
    quote::quote! {
        #[doc(hidden)]
        #[allow(all, clippy::all)] // <-- just to make sure rustc doesn't complain about generated code.
        const _: () = {
            #code
        };
    }
}

/// Returns a list of all `#[doc = "..."`] attributes found in the original list
/// of attributes.
#[cfg(feature = "native")]
pub fn doc_attributes(attrs: &[syn::Attribute]) -> Vec<syn::Attribute> {
    attrs
        .iter()
        .filter(|attr| attr.path.is_ident("doc"))
        .cloned()
        .collect::<Vec<_>>()
}

/// Iterates over all `#[doc = "..."`] attributes and concatenates their inner
/// string values, separated by newlines.
pub fn join_doc_comments(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    use syn::Lit;

    let string_literals = attrs
        .iter()
        .filter_map(|attr| attr.parse_meta().ok())
        .filter_map(|meta| match meta {
            Meta::NameValue(ref name_value) if name_value.path.is_ident("doc") => {
                Some(name_value.lit.clone())
            }
            _ => None,
        })
        .map(|lit| match lit {
            Lit::Str(s) => Ok(s.value()),
            other => Err(syn::Error::new(
                other.span(),
                "Doc comment is not a string literal",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;

    if string_literals.is_empty() {
        return Ok(None);
    }

    let trimmed: Vec<_> = string_literals
        .iter()
        .flat_map(|lit| lit.split('\n').collect::<Vec<_>>())
        .map(|line| line.trim().to_string())
        .collect();

    Ok(Some(trimmed.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_to_url_segment() {
        assert_eq!(str_to_url_segment(&format_ident!("foo")), "foo");
        assert_eq!(str_to_url_segment(&format_ident!("FOO")), "foo");
        assert_eq!(str_to_url_segment(&format_ident!("fooBar2")), "foo-bar-2");
        assert_eq!(str_to_url_segment(&format_ident!("FooBar")), "foo-bar");
    }

    #[test]
    fn get_generic_type_param_success() {
        // tests for get_generics_type_param
        let generics = syn::parse_quote! {
            <T: Trait>
        };

        let generic_param = get_generics_type_param(&generics, Span::call_site()).unwrap();
        assert_eq!(generic_param, "T");
    }

    #[test]
    fn get_generic_type_param_first_lifetime() {
        let generics = syn::parse_quote! {
            <'a, T: Trait>
        };
        let generic_param = get_generics_type_param(&generics, Span::call_site());
        let error = generic_param.unwrap_err();
        assert_eq!(error.to_string(), "Lifetime parameters are not supported.");
    }

    #[test]
    fn get_generic_type_param_first_const() {
        // error test case for get_generics_type_param when first generic param is const
        let generics = syn::parse_quote! {
            <const T: Trait>
        };
        let generic_param = get_generics_type_param(&generics, Span::call_site());

        let error = generic_param.unwrap_err();
        assert_eq!(error.to_string(), "Const parameters are not supported.");
    }
}
