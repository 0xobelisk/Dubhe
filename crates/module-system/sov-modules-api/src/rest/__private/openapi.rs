use std::collections::HashMap;

use serde_json::json;
use utoipa::openapi::OpenApi;

use super::{StateItemInfo, StateItemKind};

const OPENAPI_TEMPLATE: &str = include_str!("openapi-templates/runtime-base.yaml");

type OpenApiPaths = Vec<(String, serde_json::Value)>;

/// This should ideally be a reusable parameter defined in `#/parameters`, but
/// [`utoipa::openapi::path::Parameter`] doesn't support `$ref` at the time of
/// writing.
fn height_param() -> utoipa::openapi::path::Parameter {
    serde_json::from_value(json!({
        "name": "height",
        "in": "query",
        "description": "The block height to query the state at. Optional. If not provided, the latest block is used.",
        "required": false,
        "schema": {
            "type": "integer",
            "minimum": 0,
        }
    }))
    .unwrap()
}

/// The OpenAPI paths specification for
/// [`StateValue`](crate::containers::StateValue).
pub fn state_value_paths() -> OpenApiPaths {
    vec![(
        "/".to_string(),
        json!({
            "get": {
                "operationId": "get_state_value",
                "summary": "Get the value of a `StateValue`.",
                "parameters": [height_param()],
                "responses": {
                    "200": {
                        "$ref": "#/components/responses/StateValueResponse"
                    }
                }
            }
        }),
    )]
}

/// The OpenAPI paths specification for
/// [`StateMap`](crate::containers::StateMap).
pub fn state_map_paths() -> OpenApiPaths {
    vec![(
        "/{key}/".to_string(),
        json!({
            "get": {
                "operationId": "get_state_map",
                "summary": "Get the value of a `StateMap` element.",
                "parameters": [height_param()],
                "responses": {
                    "200": {
                        "$ref": "#/components/responses/StateMapElementResponse"
                    }
                }
            }
        }),
    )]
}

/// The OpenAPI paths specification for
/// [`StateVec`](crate::containers::StateVec).
pub fn state_vec_paths() -> OpenApiPaths {
    vec![
        (
            "/".to_string(),
            json!({
                "get": {
                    "operationId": "get_state_vec",
                    "summary": "Get general information about a `StateVec`, including its length.",
                    "parameters": [height_param()],
                    "responses": {
                        "200": {
                            "$ref": "#/components/responses/StateVecResponse"
                        }
                    }
                }
            }),
        ),
        (
            "/items/{index}".to_string(),
            json!({
                "get": {
                    "operationId": "get_state_vec_item",
                    "summary": "Get the value of a `StateVec` element.",
                    "parameters": [
                        height_param(),
                        {
                            "name": "index",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": null
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "$ref": "#/components/responses/StateVecElementResponse"
                        }
                    }
                }
            }),
        ),
    ]
}

pub fn runtime_spec(module_specs: HashMap<String, serde_json::Value>) -> OpenApi {
    let module_specs: HashMap<String, OpenApi> = module_specs
        .into_iter()
        .map(|(name, json)| (name, serde_json::from_value(json).unwrap()))
        .collect();

    let mut spec: OpenApi = serde_yaml::from_str(OPENAPI_TEMPLATE).unwrap();

    for (name, module_spec) in module_specs {
        for (path, path_item) in module_spec.paths.paths.iter() {
            spec.paths
                .paths
                .insert(format!("/modules/{}{}", name, path), path_item.clone());
        }
    }

    spec
}

pub fn module_spec(state_items: HashMap<String, StateItemInfo>) -> OpenApi {
    let mut spec = OpenApi::default();

    for (name, info) in state_items {
        let state_item_paths = match info.r#type {
            StateItemKind::StateValue => state_value_paths(),
            StateItemKind::StateVec => state_vec_paths(),
            StateItemKind::StateMap => state_map_paths(),
        };

        for path in state_item_paths {
            spec.paths.paths.insert(
                format!("/state/{}{}", name, path.0),
                serde_json::from_value(path.1).unwrap(),
            );
        }
    }

    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_template_is_valid() {
        let _: OpenApi = serde_yaml::from_str(OPENAPI_TEMPLATE).unwrap();
    }
}
