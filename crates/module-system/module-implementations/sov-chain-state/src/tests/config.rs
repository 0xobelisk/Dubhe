use sov_modules_api::da::{NanoSeconds, Time};
use sov_modules_api::prelude::serde_json;
use sov_test_utils::TestSpec;

use crate::ChainStateConfig;

#[test]
fn test_config_serialization() {
    let time = Time::new(2, NanoSeconds::new(3).unwrap());
    let config = ChainStateConfig {
        current_time: time,
        genesis_da_height: 0,
        inner_code_commitment: Default::default(),
        outer_code_commitment: Default::default(),
    };

    let data = r#"
    {
        "current_time":{
            "secs":2,
            "nanos":3
        },
        "inner_code_commitment": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "outer_code_commitment": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "genesis_da_height": 0
    }"#;

    let parsed_config: ChainStateConfig<TestSpec> = serde_json::from_str(data).unwrap();
    assert_eq!(config, parsed_config);
}
