//! When autogenerating APIs for state items, we have a few
//! requirements:
//!
//! 1. We require a "blanket" implementation on **ALL** state items,
//!    even the ones that for which we can't provide an API, for
//!    the autoref trick. Without this, compilation would fail unless
//!    all unsuitable state items were marked with `skip`. See
//!    [`StateItemRestApi`].
//! 2. We require a method that is purposefully **NOT**
//!    blanket-implemented, to cause a compilation errors when unsuitable
//!    state items are marked with `include`. See
//!    [`StateItemRestApiExists`].se std::marker::PhantomData;

use std::convert::Infallible;
use std::marker::PhantomData;

use axum::extract::State;
use axum::routing::get;
use serde::Serialize;
use sov_rest_utils::{ApiResult, Path, Query};
use sov_state::{CompileTimeNamespace, StateCodec, StateItemCodec};
use unwrap_infallible::UnwrapInfallible;

use super::types::StateItemContents;
use super::{
    maybe_archival_accessor, HeightQueryParam, ModuleSendSync, NamespacedStateMap,
    NamespacedStateVec, StateItemInfo, StorageReceiver,
};
use crate::value::NamespacedStateValue;
use crate::{ApiStateAccessor, Module, StateReader};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StateItemKind {
    StateValue,
    StateVec,
    StateMap,
}

#[derive(derivative::Derivative)]
#[derivative(Clone(bound = ""))]
pub struct StateItemRestApiImpl<M: Module, T> {
    pub storage: StorageReceiver<M::Spec>,
    pub state_item_info: StateItemInfo,
    pub phantom: PhantomData<T>,
}

pub trait StateItemRestApi {
    fn state_item_rest_api(&self) -> axum::Router<()>;
}

// "Blanket" implementation for all "unsuitable" state items e.g.
// non-`serde` compatible ones, using the autoref trick.
impl<T> StateItemRestApi for &T {
    fn state_item_rest_api(&self) -> axum::Router<()> {
        axum::Router::new()
    }
}

/// It's important for this trait to require [`StateItemRestApi`], otherwise type
/// bounds might get out of sync.
pub trait StateItemRestApiExists: StateItemRestApi {
    /// By calling this method, the proc-macro can "assert" that the type
    /// actually implements [`StateItemRestApi`].
    fn exists(&self) {}
}

impl<N, M, T, Codec> StateItemRestApiImpl<M, NamespacedStateValue<N, T, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    T: Serialize,
    Codec: StateCodec,
    Codec::ValueCodec: StateItemCodec<T>,
{
    async fn get_state_value_route(
        State(state): State<Self>,
        height_opt: Option<Query<HeightQueryParam>>,
    ) -> ApiResult<StateItemContents<T, T>> {
        let mut state_accessor = maybe_archival_accessor(
            ApiStateAccessor::<M::Spec>::new(state.storage.borrow().clone()),
            height_opt.map(|q| q.0.height),
        );

        let state_value = NamespacedStateValue::<N, T, Codec>::with_codec(
            state.state_item_info.prefix.0.clone(),
            Codec::default(),
        );

        let value = state_value.get(&mut state_accessor).unwrap_infallible();
        Ok(StateItemContents::Value { value }.into())
    }
}

impl<N, M, T, Codec> StateItemRestApi for StateItemRestApiImpl<M, NamespacedStateValue<N, T, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    T: Serialize + Send + Sync + 'static,
    Codec: StateCodec,
    Codec::ValueCodec: StateItemCodec<T>,
{
    fn state_item_rest_api(&self) -> axum::Router<()> {
        axum::Router::new()
            .route("/", get(Self::get_state_value_route))
            .with_state(self.clone())
    }
}

impl<N, M, T, Codec> StateItemRestApiImpl<M, NamespacedStateVec<N, T, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    T: Serialize,
    Codec: StateCodec,
    Codec::KeyCodec: StateItemCodec<usize>,
    Codec::ValueCodec: StateItemCodec<T> + StateItemCodec<usize>,
{
    fn checkpoint_and_vec(
        &self,
        height_opt: Option<Query<HeightQueryParam>>,
    ) -> (ApiStateAccessor<M::Spec>, NamespacedStateVec<N, T, Codec>) {
        (
            maybe_archival_accessor(
                ApiStateAccessor::new(self.storage.borrow().clone()),
                height_opt.map(|q| q.0.height),
            ),
            NamespacedStateVec::with_codec(self.state_item_info.prefix.0.clone(), Codec::default()),
        )
    }

    async fn get_state_vec_route(
        State(state): State<Self>,
        height_opt: Option<Query<HeightQueryParam>>,
    ) -> ApiResult<StateItemContents<T, T>> {
        let (mut api_state_accessor, state_vec) = Self::checkpoint_and_vec(&state, height_opt);

        let length = state_vec.len(&mut api_state_accessor).unwrap_infallible();
        Ok(StateItemContents::Vec { length }.into())
    }

    async fn get_state_vec_item_route(
        State(state): State<Self>,
        Path(item_index): Path<usize>,
        height_opt: Option<Query<HeightQueryParam>>,
    ) -> ApiResult<StateItemContents<T, T>> {
        let (mut api_state_accessor, state_vec) = Self::checkpoint_and_vec(&state, height_opt);

        let value = state_vec
            .get(item_index, &mut api_state_accessor)
            .unwrap_infallible();
        Ok(StateItemContents::VecElement {
            index: item_index,
            value,
        }
        .into())
    }
}

impl<N, M, T, Codec> StateItemRestApi for StateItemRestApiImpl<M, NamespacedStateVec<N, T, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    T: Serialize + Clone + Send + Sync + 'static,
    Codec: StateCodec,
    Codec::KeyCodec: StateItemCodec<usize>,
    Codec::ValueCodec: StateItemCodec<T> + StateItemCodec<usize>,
{
    fn state_item_rest_api(&self) -> axum::Router<()> {
        axum::Router::new()
            .route("/", get(Self::get_state_vec_route))
            .route("/items/:index/", get(Self::get_state_vec_item_route))
            .with_state(self.clone())
    }
}

impl<N, M, K, V, Codec> StateItemRestApiImpl<M, NamespacedStateMap<N, K, V, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    K: Serialize + serde::de::DeserializeOwned,
    V: Serialize,
    Codec: StateCodec,
    Codec::KeyCodec: StateItemCodec<K>,
    Codec::ValueCodec: StateItemCodec<V>,
{
    async fn get_state_map_route(State(state): State<Self>) -> ApiResult<StateItemInfo> {
        Ok(StateItemInfo {
            r#type: StateItemKind::StateMap,
            prefix: state.state_item_info.prefix,
            description: state.state_item_info.description.clone(),
            name: state.state_item_info.name.clone(),
            namespace: state.state_item_info.namespace.clone(),
        }
        .into())
    }

    async fn get_state_map_item_route(
        State(state): State<Self>,
        Path(key): Path<K>,
        height_opt: Option<Query<HeightQueryParam>>,
    ) -> ApiResult<StateItemContents<K, V>> {
        let mut working_set = maybe_archival_accessor(
            ApiStateAccessor::<M::Spec>::new(state.storage.borrow().clone()),
            height_opt.map(|q| q.0.height),
        );
        let state_map = NamespacedStateMap::<N, K, V, Codec>::with_codec(
            state.state_item_info.prefix.0.clone(),
            Codec::default(),
        );

        let value = state_map.get(&key, &mut working_set).unwrap_infallible();
        Ok(StateItemContents::MapElement { key, value }.into())
    }
}

impl<N, M, K, V, Codec> StateItemRestApi
    for StateItemRestApiImpl<M, NamespacedStateMap<N, K, V, Codec>>
where
    N: CompileTimeNamespace,
    M: ModuleSendSync,
    ApiStateAccessor<M::Spec>: StateReader<N, Error = Infallible>,
    K: Serialize + serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
    V: Serialize + Clone + Send + Sync + 'static,
    Codec: StateCodec,
    Codec::KeyCodec: StateItemCodec<K>,
    Codec::ValueCodec: StateItemCodec<V>,
{
    fn state_item_rest_api(&self) -> axum::Router<()> {
        axum::Router::new()
            .route("/", get(Self::get_state_map_route))
            .route("/items/:key", get(Self::get_state_map_item_route))
            .with_state(self.clone())
    }
}

impl<N, M, T, Codec> StateItemRestApiExists
    for StateItemRestApiImpl<M, NamespacedStateValue<N, T, Codec>>
where
    M: Module,
    Self: StateItemRestApi,
{
}

impl<N, M, T, Codec> StateItemRestApiExists
    for StateItemRestApiImpl<M, NamespacedStateVec<N, T, Codec>>
where
    M: Module,
    Self: StateItemRestApi,
{
}

impl<N, M, K, V, Codec> StateItemRestApiExists
    for StateItemRestApiImpl<M, NamespacedStateMap<N, K, V, Codec>>
where
    M: Module,
    Self: StateItemRestApi,
{
}

pub trait GetStateItemInfo {
    const STATE_ITEM_KIND: StateItemKind;
    const NAMESPACE: sov_state::Namespace;
}

impl<N, V, Codec> GetStateItemInfo for NamespacedStateValue<N, V, Codec>
where
    N: CompileTimeNamespace,
{
    const STATE_ITEM_KIND: StateItemKind = StateItemKind::StateValue;
    const NAMESPACE: sov_state::Namespace = N::NAMESPACE;
}

impl<N, V, Codec> GetStateItemInfo for NamespacedStateVec<N, V, Codec>
where
    N: CompileTimeNamespace,
{
    const STATE_ITEM_KIND: StateItemKind = StateItemKind::StateVec;
    const NAMESPACE: sov_state::Namespace = N::NAMESPACE;
}

impl<N, K, V, Codec> GetStateItemInfo for NamespacedStateMap<N, K, V, Codec>
where
    N: CompileTimeNamespace,
{
    const STATE_ITEM_KIND: StateItemKind = StateItemKind::StateMap;
    const NAMESPACE: sov_state::Namespace = N::NAMESPACE;
}
