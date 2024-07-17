use std::env;

fn set_constants_manifest() {
    env::set_var("CONSTANTS_MANIFEST_TEST_MODE", "1");
}

#[test]
fn module_info_tests() {
    set_constants_manifest();
    let t = trybuild::TestCases::new();
    t.pass("tests/integration/module_info/parse.rs");
    t.pass("tests/integration/module_info/mod_and_state.rs");
    t.pass("tests/integration/module_info/use_address_trait.rs");
    t.pass("tests/integration/module_info/not_supported_attribute.rs");
    t.pass("tests/integration/module_info/custom_codec_builder.rs");
    t.pass("tests/integration/custom_codec_must_be_used.rs");
    t.compile_fail("tests/integration/module_info/derive_on_enum_not_supported.rs");
    t.compile_fail("tests/integration/module_info/field_missing_attribute.rs");
    t.compile_fail("tests/integration/module_info/missing_address.rs");
    t.compile_fail("tests/integration/module_info/no_generics.rs");
    t.compile_fail("tests/integration/module_info/not_supported_type.rs");
    t.compile_fail("tests/integration/module_info/second_addr_not_supported.rs");
}

#[test]
fn module_dispatch_tests() {
    set_constants_manifest();
    let t = trybuild::TestCases::new();
    t.pass("tests/integration/dispatch/derive_genesis.rs");
    t.pass("tests/integration/dispatch/derive_dispatch.rs");
    t.pass("tests/integration/dispatch/derive_event.rs");
    t.compile_fail("tests/integration/dispatch/missing_serialization.rs");
}

#[test]
fn rpc_tests() {
    set_constants_manifest();
    let t = trybuild::TestCases::new();
    t.pass("tests/integration/rpc/derive_rpc.rs");
    t.pass("tests/integration/rpc/expose_rpc.rs");
    t.pass("tests/integration/rpc/expose_rpc_associated_types.rs");
    t.pass("tests/integration/rpc/expose_rpc_associated_types_nested.rs");
    t.pass("tests/integration/rpc/derive_rpc_without_working_set_deny_missing_docs.rs");

    t.compile_fail("tests/integration/rpc/derive_rpc_working_set_immutable_reference.rs");
    t.compile_fail("tests/integration/rpc/derive_rpc_working_set_no_generic.rs");
    t.compile_fail("tests/integration/rpc/expose_rpc_associated_type_not_static.rs");
    t.compile_fail("tests/integration/rpc/expose_rpc_first_generic_not_spec.rs");
}

#[test]
fn cli_wallet_arg_tests() {
    set_constants_manifest();
    let t: trybuild::TestCases = trybuild::TestCases::new();

    t.pass("tests/integration/cli_wallet_arg/derive_enum_named_fields.rs");
    t.pass("tests/integration/cli_wallet_arg/derive_struct_unnamed_fields.rs");
    t.pass("tests/integration/cli_wallet_arg/derive_struct_named_fields.rs");
    t.pass("tests/integration/cli_wallet_arg/derive_enum_mixed_fields.rs");
    t.pass("tests/integration/cli_wallet_arg/derive_enum_unnamed_fields.rs");
    t.pass("tests/integration/cli_wallet_arg/derive_wallet.rs");
}

#[test]
fn constants_from_manifests_test() {
    set_constants_manifest();
    let t: trybuild::TestCases = trybuild::TestCases::new();

    // TOOD: Add compile fail on address prefix and mismatched prefix and invalid bech3
    t.pass("tests/integration/constants/create_constant.rs");
    t.compile_fail("tests/integration/constants/bech32_constant_invalid_checksum.rs");
    t.compile_fail("tests/integration/constants/bech32_constant_prefix_too_short.rs");
    t.compile_fail("tests/integration/constants/bech32_constant_prefix_too_long.rs");
    t.compile_fail("tests/integration/constants/bech32_constant_not_a_string.rs");
}
