use std::path::{Path, PathBuf};
use std::{fmt, fs};

use proc_macro2::{Ident, TokenStream};
use syn::{PathArguments, Type, TypePath};
use toml::Value;

use crate::common::toml_value_to_expr;

const CONSTANTS_MANIFEST_PATH: Option<&str> = option_env!("CONSTANTS_MANIFEST_PATH");

#[derive(Debug, Clone)]
pub struct Manifest<'a> {
    parent: &'a Ident,
    path: PathBuf,
    value: Value,
}

impl<'a> Manifest<'a> {
    /// Parse a manifest file from a string.
    ///
    /// The provided path will be used to feedback error to the user, if any.
    ///
    /// The `parent` is used to report the errors to the correct span location.
    pub fn read_str<S>(manifest: S, path: PathBuf, parent: &'a Ident) -> Result<Self, syn::Error>
    where
        S: AsRef<str>,
    {
        let value = toml::from_str(manifest.as_ref())
            .map_err(|e| Self::err(&path, parent, format!("failed to parse manifest: {e}")))?;

        Ok(Self {
            parent,
            path,
            value,
        })
    }

    /// Reads a `constants.toml` manifest file, walking up the directory tree
    /// starting from
    /// [`OUT_DIR`](https://doc.rust-lang.org/cargo/reference/environment-variables.html) until it finds
    /// one.
    ///
    /// If the environment variable `CONSTANTS_MANIFEST` is set, the file will
    /// be read from that directory instead.
    ///
    /// If the `test` Cargo feature is enabled or the environment variable
    /// `CONSTANTS_MANIFEST_TEST_MODE` is set, the proc-macro will look for a
    /// file named `constants.testing.toml` instead.
    ///
    /// # Arguments
    ///
    /// `parent` is used to report the errors to the correct span location.
    pub fn read_constants(parent: &'a Ident) -> syn::Result<Self> {
        let constants_path = CONSTANTS_MANIFEST_PATH.map(PathBuf::from).ok_or_else(|| {
            syn::Error::new(
                parent.span(),
                format!(
                    "Failed to find a `{}` file in the current directory or any parent directory",
                    "constants.toml"
                ),
            )
        })?;

        let constants = fs::read_to_string(&constants_path).map_err(|e| {
            Self::err(
                &constants_path,
                parent,
                format!("failed to read `{}`: {}", constants_path.display(), e),
            )
        })?;

        Self::read_str(constants, constants_path, parent)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Gets the requested object from the manifest by key
    fn get_object(&self, field: &Ident, key: &str) -> syn::Result<&toml::Table> {
        self.value
            .as_table()
            .ok_or_else(|| Self::err(&self.path, field, "manifest is not an object"))?
            .get(key)
            .ok_or_else(|| {
                Self::err(
                    &self.path,
                    field,
                    format!("manifest does not contain a `{key}` attribute"),
                )
            })?
            .as_table()
            .ok_or_else(|| {
                Self::err(
                    &self.path,
                    field,
                    format!("`{key}` attribute of `{field}` is not a table"),
                )
            })
    }

    /// Parses a gas config constant from the manifest file. Returns a `TokenStream` with the
    /// following structure:
    ///
    /// ```rust,ignore
    /// const GAS_CONFIG: Self::GasConfig = Self::GasConfig {
    ///     foo: [1u64, 2u64, 3u64, ],
    ///     bar: [4u64, 5u64, 6u64, ],
    /// };
    /// ```
    ///
    /// Where `foo` and `bar` are fields of the TOML constants file under the located `gas` field.
    ///
    /// The `gas` field resolution will first attempt to query `gas.parent`, and then fallback to
    /// `gas`. They must be objects with arrays of integers as fields.
    pub fn parse_gas_config(&self, ty: &Type, field: &Ident) -> Result<TokenStream, syn::Error> {
        let root = self.get_object(field, "gas")?;

        let root = match root.get(&self.parent.to_string()) {
            Some(Value::Table(t)) => t,
            Some(_) => {
                return Err(Self::err(
                    &self.path,
                    field,
                    format!("matching constants entry `{}` is not an object", field),
                ))
            }
            None => root,
        };

        let mut field_values = vec![];
        for (k, v) in root {
            let k: Ident = syn::parse_str(k).map_err(|e| {
                Self::err(
                    &self.path,
                    field,
                    format!("failed to parse key attribute `{}`: {}", k, e),
                )
            })?;

            let v = match v {
                Value::Array(a) => a
                    .iter()
                    .map(|v| match v {
                        Value::Boolean(b) => Ok(*b as u64),
                        Value::Integer(n) => Ok(u64::try_from(*n).map_err(|_| {
                            Self::err(
                                &self.path,
                                field,
                                format!(
                                    "the value of the field `{k}` must be an array of valid `u64`"
                                ),
                            )
                        })?),
                        _ => Err(Self::err(
                            &self.path,
                            field,
                            format!(
                            "the value of the field `{k}` must be an array of numbers, or booleans"
                        ),
                        )),
                    })
                    .collect::<Result<_, _>>()?,
                Value::Integer(n) => vec![u64::try_from(*n).map_err(|_| {
                    Self::err(
                        &self.path,
                        field,
                        format!("the value of the field `{k}` must be a `u64`"),
                    )
                })?],
                Value::Boolean(b) => vec![*b as u64],

                _ => {
                    return Err(Self::err(
                        &self.path,
                        field,
                        format!(
                            "the value of the field `{k}` must be an array, number, or boolean"
                        ),
                    ))
                }
            };

            field_values.push(quote::quote!(#k: <<<Self as ::sov_modules_api::Module>::Spec as ::sov_modules_api::Spec>::Gas as ::sov_modules_api::GasArray>::from_slice(&[#(#v,)*])));
        }

        // remove generics, if any
        let mut ty = ty.clone();
        if let Type::Path(TypePath { path, .. }) = &mut ty {
            if let Some(p) = path.segments.last_mut() {
                p.arguments = PathArguments::None;
            }
        }

        Ok(quote::quote! {
            let #field = #ty {
                #(#field_values,)*
            };
        })
    }

    pub fn parse_expression(&self, field: &Ident) -> Result<TokenStream, syn::Error> {
        let root = self.get_object(field, "constants")?;
        let value = root.get(&field.to_string()).ok_or_else(|| {
            Self::err(
                &self.path,
                field,
                format!("manifest does not contain a `{}` attribute", field),
            )
        })?;

        let expr = toml_value_to_expr(value, field.span())?;
        Ok(quote::quote!(#expr))
    }

    fn err<P, T>(path: P, ident: &syn::Ident, msg: T) -> syn::Error
    where
        P: AsRef<Path>,
        T: fmt::Display,
    {
        syn::Error::new(
            ident.span(),
            format!(
                "failed to parse manifest `{}` for `{}`: {}",
                path.as_ref().display(),
                ident,
                msg
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gas_config_works() {
        let input = r#"
            [gas]
            complex_math_operation = [1, 2, 3]
            some_other_operation = [4, 5, 6]
        "#;

        let parent = Ident::new("Foo", proc_macro2::Span::call_site());
        let gas_config: Type = syn::parse_str("FooGasConfig<S::Gas>").unwrap();
        let field: Ident = syn::parse_str("foo_gas_config").unwrap();

        let decl = Manifest::read_str(input, PathBuf::from("foo.toml"), &parent)
            .unwrap()
            .parse_gas_config(&gas_config, &field)
            .unwrap();

        #[rustfmt::skip]
        assert_eq!(
            decl.to_string(),
            quote::quote!(
                let foo_gas_config = FooGasConfig {
                    complex_math_operation: <<<Self as ::sov_modules_api::Module>::Spec as  ::sov_modules_api::Spec>::Gas as ::sov_modules_api::GasArray>::from_slice(&[1u64, 2u64, 3u64, ]),
                    some_other_operation: <<<Self as ::sov_modules_api::Module>::Spec as  ::sov_modules_api::Spec>::Gas as ::sov_modules_api::GasArray>::from_slice(&[4u64, 5u64, 6u64, ]),
                };
            )
            .to_string()
        );
    }

    #[test]
    fn parse_gas_config_single_dimension_works() {
        let input = r#"
            [gas]
            complex_math_operation = 1
            some_other_operation = 2
        "#;

        let parent = Ident::new("Foo", proc_macro2::Span::call_site());
        let gas_config: Type = syn::parse_str("FooGasConfig<S::Gas>").unwrap();
        let field: Ident = syn::parse_str("foo_gas_config").unwrap();

        let decl = Manifest::read_str(input, PathBuf::from("foo.toml"), &parent)
            .unwrap()
            .parse_gas_config(&gas_config, &field)
            .unwrap();

        #[rustfmt::skip]
        assert_eq!(
            decl.to_string(),
            quote::quote!(
                let foo_gas_config = FooGasConfig {
                    complex_math_operation: <<<Self as ::sov_modules_api::Module>::Spec as  ::sov_modules_api::Spec>::Gas as ::sov_modules_api::GasArray>::from_slice(&[1u64, ]),
                    some_other_operation: <<<Self as ::sov_modules_api::Module>::Spec as  ::sov_modules_api::Spec>::Gas as ::sov_modules_api::GasArray>::from_slice(&[2u64, ]),
                };
            ).to_string()
        );
    }
}
