use sov_modules_api::utils::generate_address as gen_addr_generic;
use sov_modules_api::Spec;
use sov_test_utils::TestSpec;

use crate::NonFungibleTokenConfig;

#[test]
fn test_config_serialization() {
    let address: <TestSpec as Spec>::Address = gen_addr_generic::<TestSpec>("admin");
    let owner: <TestSpec as Spec>::Address = gen_addr_generic::<TestSpec>("owner");

    let config = NonFungibleTokenConfig::<TestSpec> {
        admin: address,
        owners: vec![(0, owner)],
    };

    let data = r#"
    {
        "admin":"sov1335hded4gyzpt00fpz75mms4m7ck02wgw07yhw9grahj4dzg4yvqk63pml",
        "owners":[
            [0,"sov1fsgzj6t7udv8zhf6zj32mkqhcjcpv52yph5qsdcl0qt94jgdckqsczjm2y"]
        ]
    }"#;

    let parsed_config: NonFungibleTokenConfig<TestSpec> = serde_json::from_str(data).unwrap();
    assert_eq!(config, parsed_config)
}
