pub mod openapi;
pub mod state;
mod types;

use sov_rest_utils::ApiResult;

use self::state::StateItemKind;
use self::types::{ModuleObject, Namespace};
use super::*;
use crate::ApiStateAccessor;

/// Trait "alias" for simpler trait bounds.
pub trait ModuleSendSync: Module + Send + Sync + 'static {}
impl<M> ModuleSendSync for M where M: Module + Send + Sync + 'static {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateItemInfo {
    pub r#type: StateItemKind,
    #[serde(skip)]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub namespace: Namespace,
    pub prefix: Prefix,
}

pub fn maybe_archival_accessor<S: Spec>(
    api_state_accessor: ApiStateAccessor<S>,
    height_opt: Option<u64>,
) -> ApiStateAccessor<S> {
    if let Some(height) = height_opt {
        api_state_accessor.get_archival_at(height)
    } else {
        api_state_accessor
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeObject {
    modules: HashMap<String, ModuleOverview>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleOverview {
    pub id: ModuleId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Prefix(pub sov_state::Prefix);

impl Serialize for Prefix {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = format!("0x{}", hex::encode(&self.0));
        serializer.serialize_str(&s)
    }
}

/// A basic implementor of [`HasRestApi`] for a runtime.
///
/// The resulting [`axum::Router`] should then be merged with runtime-specific
/// routes, e.g. for each child module.
///
/// We can safely assume that all runtimes implement [`TxHooks`], which
/// happens to expose an associated type [`Spec`] and, as such, is a
/// great way for a proc-macro to "get" the runtime's [`Spec`] without
/// having to guess which generic parameter it is.
#[derive(derivative::Derivative)]
#[derivative(Clone(bound = ""))]
pub struct RuntimeRestApiBaseImpl<R: TxHooks> {
    pub runtime: Arc<R>,
    pub modules: HashMap<String, ModuleOverview>,
}

impl<R> RuntimeRestApiBaseImpl<R>
where
    R: TxHooks + Send + Sync + 'static,
{
    async fn root_handler(State(state): State<Self>) -> ApiResult<RuntimeObject> {
        Ok(RuntimeObject {
            modules: state.modules.clone(),
        }
        .into())
    }
}

impl<R> HasRestApi<R::Spec> for RuntimeRestApiBaseImpl<R>
where
    R: TxHooks + Send + Sync + 'static,
{
    fn rest_api(&self, _storage: StorageReceiver<R::Spec>) -> axum::Router<()> {
        axum::Router::new()
            .route("/", get(Self::root_handler))
            .with_state(self.clone())
            .fallback(sov_rest_utils::errors::global_404)
    }
}

/// A basic implementor of [`HasRestApi`] for a module.
///
/// The resulting [`axum::Router`] should then be merged with module-specific
/// routes, e.g. for each state item.
#[derive(Clone)]
pub struct ModuleRestApiBaseImpl<M: Module> {
    pub module: Arc<M>,
    pub description: Option<String>,
    pub storage: StorageReceiver<M::Spec>,
    pub state_items: HashMap<String, StateItemInfo>,
}

impl<M> HasRestApi<<M as Module>::Spec> for ModuleRestApiBaseImpl<M>
where
    M: ModuleSendSync + ModuleInfo + Clone,
{
    fn rest_api(&self, _storage: StorageReceiver<<M as Module>::Spec>) -> axum::Router<()> {
        axum::Router::new()
            .route("/", get(Self::root_route))
            .with_state(self.clone())
    }
}

impl<M> ModuleRestApiBaseImpl<M>
where
    M: ModuleSendSync + ModuleInfo + Clone,
{
    /// The handler function for the root path of the router, which
    /// returns some general information about the module (name, ID,
    /// etc.).
    async fn root_route(State(state): State<Self>) -> ApiResult<ModuleObject> {
        Ok(ModuleObject::new(
            &*state.module,
            state.description.clone(),
            state.state_items.clone(),
        )
        .into())
    }
}
