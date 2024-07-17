use std::collections::HashMap;
use std::fmt::Display;
use std::marker::PhantomData;
use std::ops::Range;
use std::sync::OnceLock;

use anyhow::Context;
use axum::extract::ws::WebSocket;
use axum::extract::{ws, Request, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{middleware, Extension};
use borsh::{BorshDeserialize, BorshSerialize};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sov_db::schema::types::{BatchNumber, EventNumber, SlotNumber, TxNumber};
use sov_modules_api::{EventModuleName, RuntimeEventResponse};
use sov_rest_utils::errors::{
    self, database_error_response_500, internal_server_error_response_500, not_found_404,
};
use sov_rest_utils::{json_obj, preconfigured_router_layers, ApiResult, ErrorObject, Path, Query};
use sov_rollup_interface::common::{HexHash, HexString};
use sov_rollup_interface::rpc::{
    AggregatedProofResponse, BatchIdAndOffset, BatchIdentifier, BatchResponse, EventIdentifier,
    FinalityStatus, ItemOrHash, LedgerStateProvider, QueryMode, SlotIdAndOffset, SlotIdentifier,
    SlotResponse, TxIdAndOffset, TxIdentifier, TxResponse,
};
use sov_rollup_interface::stf::TxReceiptContents;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};
use tracing::warn;
use utoipa_swagger_ui::{Config, SwaggerUi};

type PathMap = Path<HashMap<String, NumberOrHash>>;

/// This function does a pretty expensive clone of the entire OpenAPI
/// specification object, so it might be slow.
pub(crate) fn openapi_spec() -> serde_json::Value {
    static OPENAPI_SPEC: OnceLock<serde_json::Value> = OnceLock::new();

    OPENAPI_SPEC
        .get_or_init(|| {
            let openapi_spec_raw_yaml_contents = include_str!("../openapi-v3.yaml");
            serde_yaml::from_str::<serde_json::Value>(openapi_spec_raw_yaml_contents).unwrap()
        })
        .clone()
}

/// Error to be returned when our bespoke path captures parser fails.
fn bad_path_error(key: &str) -> Response {
    ErrorObject {
        status: StatusCode::BAD_REQUEST,
        title: "Bad request".to_string(),
        details: json_obj!({
            "message": format!("{} is missing or invalid", key),
        }),
    }
    .into_response()
}

/// Finds a specific path component in a [`PathMap`] of type [`NumberOrHash`].
fn get_path_item(path_map: &PathMap, key: &str) -> Result<NumberOrHash, Response> {
    if let Some(value) = path_map.get(key) {
        Ok(*value)
    } else {
        Err(bad_path_error(key))
    }
}

/// Finds a specific path component in a [`PathMap`] of type [`u64`]. Used for
/// parsing offsets.
fn get_path_number(path_map: &PathMap, key: &str) -> Result<u64, Response> {
    if let Some(value) = path_map.get(key).and_then(|value| value.as_u64()) {
        Ok(value)
    } else {
        Err(bad_path_error(key))
    }
}

/// Use [`LedgerRoutes::axum_router`] to instantiate an [`axum::Router`] for
/// a specific [`LedgerStateProvider`].
///
/// This `struct` simply serves as a grouping of generics to reduce code
/// verbosity.
pub struct LedgerRoutes<T, B, Tx, E> {
    phantom: PhantomData<(T, B, Tx, E)>,
}

impl<T, B, TxReceipt, E> LedgerRoutes<T, B, TxReceipt, E>
where
    T: LedgerStateProvider + Clone + Send + Sync + 'static,
    B: serde::Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
    TxReceipt: TxReceiptContents,
    E: EventModuleName
        + serde::Serialize
        + serde::de::DeserializeOwned
        + BorshSerialize
        + BorshDeserialize
        + Clone
        + Send
        + Sync
        + 'static,
{
    /// Returns an [`axum::Router`] that exposes ledger data.
    pub fn axum_router(ledger: T, path_prefix: &str) -> axum::Router<T> {
        preconfigured_router_layers(
            axum::Router::<T>::new()
                // See:
                // - https://github.com/juhaku/utoipa/issues/599
                // - https://github.com/juhaku/utoipa/issues/734
                .merge(
                    SwaggerUi::new("/swagger-ui")
                        .external_url_unchecked("/openapi-v3.yaml", openapi_spec())
                        .config(Config::from(format!("{}/openapi-v3.yaml", path_prefix))),
                )
                .route(
                    "/aggregated-proofs/latest",
                    get(Self::get_latest_aggregated_proof),
                )
                .route(
                    "/aggregated-proofs/latest/ws",
                    get(Self::subscribe_to_aggregated_proofs),
                )
                .route("/slots/latest/ws", get(Self::subscribe_to_head))
                .route("/slots/finalized/ws", get(Self::subscribe_to_finalized))
                .nest(
                    "/slots/latest",
                    Self::router_slot(ledger.clone()).route_layer(middleware::from_fn_with_state(
                        ledger.clone(),
                        Self::resolve_latest_slot,
                    )),
                )
                .nest(
                    "/slots/:slotId",
                    Self::router_slot(ledger.clone()).route_layer(middleware::from_fn_with_state(
                        ledger.clone(),
                        Self::resolve_slot_id,
                    )),
                )
                .nest(
                    "/batches/:batchId",
                    Self::router_batch(ledger.clone()).route_layer(middleware::from_fn_with_state(
                        ledger.clone(),
                        Self::resolve_batch_id,
                    )),
                )
                .nest(
                    "/txs/:txId",
                    Self::router_tx(ledger.clone()).route_layer(middleware::from_fn_with_state(
                        ledger.clone(),
                        Self::resolve_tx_id,
                    )),
                )
                .nest(
                    "/events/:eventId",
                    Self::router_event().route_layer(middleware::from_fn_with_state(
                        ledger,
                        Self::resolve_event_id,
                    )),
                ),
        )
    }

    // ROUTERS
    // -------
    // The following routers are not the typical routers that you'd find in
    // Axum examples. This is because they compose with each other through
    // nesting and layering to offer easy access to sub-resources using
    // nested routes, e.g.:
    //
    // - /slots/latest
    // - /slots/latest/batches/2
    // - /txs/0x1337/events/42

    fn router_slot(ledger: T) -> axum::Router<T> {
        axum::Router::new()
            .route("/", get(Self::get_slot))
            .nest(
                "/batches/:batchOffset",
                Self::router_batch(ledger.clone()).layer(middleware::from_fn_with_state(
                    ledger.clone(),
                    Self::resolve_batch_offset,
                )),
            )
            .route("/events", get(Self::get_slot_events))
    }

    fn router_batch(ledger: T) -> axum::Router<T> {
        axum::Router::new().route("/", get(Self::get_batch)).nest(
            "/txs/:txOffset",
            Self::router_tx(ledger.clone()).layer(middleware::from_fn_with_state(
                ledger.clone(),
                Self::resolve_tx_offset,
            )),
        )
    }

    fn router_tx(ledger: T) -> axum::Router<T> {
        axum::Router::new()
            .route("/", get(Self::get_tx))
            .route("/events", get(Self::get_tx_events))
            .nest(
                "/events/:eventOffset",
                Self::router_event().layer(middleware::from_fn_with_state(
                    ledger,
                    Self::resolve_event_offset,
                )),
            )
    }

    fn router_event() -> axum::Router<T> {
        axum::Router::new().route("/", get(Self::get_event))
    }

    // HANDLERS
    // --------
    // Most of these handlers rely on "extension" values set by the
    // middleware functions at different nesting levels. You'll need to
    // carefully inspect the routers to see how they are set.

    async fn get_slot(
        State(ledger): State<T>,
        include_children_opt: Option<Query<IncludeChildren>>,
        Extension(SlotNumber(slot_number)): Extension<SlotNumber>,
    ) -> ApiResult<Slot<B, TxReceipt, E>> {
        match ledger
            .get_slot_by_number::<B, TxReceipt>(
                slot_number,
                include_children_opt.map(|q| q.0).unwrap_or_default().into(),
            )
            .await
        {
            Ok(Some(slot_response)) => Ok(Slot::new(slot_response).into()),
            Ok(None) => Err(errors::not_found_404("Slot", slot_number)),
            Err(err) => Err(errors::database_error_response_500(err)),
        }
    }

    async fn get_slot_events(
        State(ledger): State<T>,
        Extension(SlotNumber(slot_number)): Extension<SlotNumber>,
        event_key_prefix_opt: Option<Query<EventFilter>>,
    ) -> ApiResult<Vec<Event<E>>> {
        let filter = event_key_prefix_opt.map(|q| q.0.prefix.into());
        let events = ledger
            .get_filtered_slot_events::<B, TxReceipt, RuntimeEventResponse<E>>(
                &SlotIdentifier::Number(slot_number),
                filter,
            )
            .await
            .map_err(database_error_response_500)?;

        Ok(events
            .into_iter()
            .map(|e| Event {
                number: e.event_number,
                key: e.event_key,
                value: e.event_value,
                module: ModuleRef {
                    name: e.module_name,
                },
            })
            .collect::<Vec<_>>()
            .into())
    }

    async fn get_batch(
        State(ledger): State<T>,
        include_children_opt: Option<Query<IncludeChildren>>,
        Extension(BatchNumber(batch_number)): Extension<BatchNumber>,
    ) -> ApiResult<Batch<B, TxReceipt, E>> {
        match ledger
            .get_batch_by_number::<B, TxReceipt>(
                batch_number,
                include_children_opt.map(|q| q.0).unwrap_or_default().into(),
            )
            .await
        {
            Ok(Some(batch_response)) => Ok(Batch::new(batch_response, batch_number).into()),
            Ok(None) => Err(errors::not_found_404("Batch", batch_number)),
            Err(err) => Err(errors::database_error_response_500(err)),
        }
    }

    async fn get_tx(
        State(ledger): State<T>,
        include_children_opt: Option<Query<IncludeChildren>>,
        Extension(TxNumber(tx_number)): Extension<TxNumber>,
    ) -> ApiResult<Transaction<TxReceipt, E>> {
        match ledger
            .get_tx_by_number::<TxReceipt>(
                tx_number,
                include_children_opt.map(|q| q.0).unwrap_or_default().into(),
            )
            .await
        {
            Ok(Some(tx_response)) => Ok(Transaction::new(tx_response, tx_number).into()),
            Ok(None) => Err(errors::not_found_404("Transaction", tx_number)),
            Err(err) => Err(errors::database_error_response_500(err)),
        }
    }

    async fn get_tx_events(
        State(ledger): State<T>,
        Extension(TxNumber(tx_number)): Extension<TxNumber>,
        event_key_prefix_opt: Option<Query<EventFilter>>,
    ) -> ApiResult<Vec<Event<E>>> {
        match ledger
            .get_events_by_txn_number::<RuntimeEventResponse<E>>(tx_number)
            .await
        {
            Ok(events) => Ok(events
                .into_iter()
                .filter(|event| {
                    if let Some(prefix) = &event_key_prefix_opt {
                        event.event_key.starts_with(&prefix.prefix)
                    } else {
                        true
                    }
                })
                .map(|e| Event {
                    number: e.event_number,
                    key: e.event_key,
                    value: e.event_value,
                    module: ModuleRef {
                        name: e.module_name,
                    },
                })
                .collect::<Vec<_>>()
                .into()),
            Err(err) => Err(errors::database_error_response_500(err)),
        }
    }

    async fn get_event(
        State(ledger): State<T>,
        Extension(EventNumber(event_number)): Extension<EventNumber>,
    ) -> ApiResult<Event<E>> {
        match ledger
            .get_event_by_number::<RuntimeEventResponse<E>>(event_number)
            .await
        {
            Ok(Some(event_response)) => Ok(Event {
                number: event_number,
                key: event_response.event_key,
                value: event_response.event_value,
                module: ModuleRef {
                    name: event_response.module_name,
                },
            }
            .into()),
            Ok(None) => Err(errors::not_found_404("Event", event_number)),
            Err(err) => Err(errors::database_error_response_500(err)),
        }
    }

    // ENTITY ID RESOLVERS
    // -------------------
    // These are middleware functions that resolve the entity ID (i.e.
    // numbers) or the "root" entity, given its hash or number.

    async fn resolve_latest_slot(
        State(ledger): State<T>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let latest_slot = ledger
            .get_head_slot_number()
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Slot", "latest"))?;

        request.extensions_mut().insert(SlotNumber(latest_slot));
        Ok(next.run(request).await)
    }

    async fn resolve_slot_id(
        State(ledger): State<T>,
        path_values: PathMap,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let identifier = match get_path_item(&path_values, "slotId")? {
            NumberOrHash::Number(number) => SlotIdentifier::Number(number),
            NumberOrHash::Hash(hash) => SlotIdentifier::Hash(hash.0),
        };

        let slot_number = ledger
            .resolve_slot_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            // TODO: 404s should *NOT* be generated with entity IDs set to
            // "unknown", it's bad UX. Unfortunately, we need the identifier to
            // implement `ToString`, and the identifier types that we're using
            // at the time of writing don't. While we could map them into
            // `NumberOrHash` just for better error messaging, it will
            // complicate the code. Once we remove JSON-RPC identifier types, we
            // can remove this workaround and do the right thing.
            .ok_or_else(|| not_found_404("Slot", "unknown"))?;

        request.extensions_mut().insert(SlotNumber(slot_number));
        Ok(next.run(request).await)
    }

    async fn resolve_batch_id(
        path_values: PathMap,
        State(ledger): State<T>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let identifier = match get_path_item(&path_values, "batchId")? {
            NumberOrHash::Number(number) => BatchIdentifier::Number(number),
            NumberOrHash::Hash(hash) => BatchIdentifier::Hash(hash.0),
        };
        let batch_number = ledger
            .resolve_batch_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Batch", "unknown"))?;

        request.extensions_mut().insert(BatchNumber(batch_number));
        Ok(next.run(request).await)
    }

    async fn resolve_tx_id(
        State(ledger): State<T>,
        path_values: PathMap,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let identifier = match get_path_item(&path_values, "txId")? {
            NumberOrHash::Number(number) => TxIdentifier::Number(number),
            NumberOrHash::Hash(hash) => TxIdentifier::Hash(hash.0),
        };
        let tx_number = ledger
            .resolve_tx_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Transaction", "unknown"))?;

        request.extensions_mut().insert(TxNumber(tx_number));
        Ok(next.run(request).await)
    }

    async fn resolve_event_id(
        State(ledger): State<T>,
        path_values: PathMap,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        // Events can't be resolved by hash, only by number.
        let identifier = EventIdentifier::Number(get_path_number(&path_values, "eventId")?);

        let event_number = ledger
            .resolve_event_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Event", "unknown"))?;

        request.extensions_mut().insert(EventNumber(event_number));
        Ok(next.run(request).await)
    }

    // ENTITY ID RESOLVERS BY OFFSET
    // -----------------------------
    // These are middleware functions that resolve some entity ID based on
    // the parent entity ID and the child's offset.
    //
    // No need for a resolved by offset for slots, because they have no
    // parent entity.

    async fn resolve_batch_offset(
        State(ledger): State<T>,
        path_values: PathMap,
        Extension(slot_number): Extension<SlotNumber>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let batch_offset = get_path_number(&path_values, "batchOffset")?;

        let identifier = BatchIdentifier::SlotIdAndOffset(SlotIdAndOffset {
            slot_id: SlotIdentifier::Number(slot_number.0),
            offset: batch_offset,
        });
        let batch_number = ledger
            .resolve_batch_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Batch", batch_offset))?;

        request.extensions_mut().insert(BatchNumber(batch_number));
        Ok(next.run(request).await)
    }

    async fn resolve_tx_offset(
        State(ledger): State<T>,
        path_values: PathMap,
        Extension(batch_number): Extension<BatchNumber>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let tx_offset = get_path_number(&path_values, "txOffset")?;
        let identifier = TxIdentifier::BatchIdAndOffset(BatchIdAndOffset {
            batch_id: BatchIdentifier::Number(batch_number.0),
            offset: tx_offset,
        });

        let tx_number = ledger
            .resolve_tx_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Transaction", tx_offset))?;

        request.extensions_mut().insert(TxNumber(tx_number));
        Ok(next.run(request).await)
    }

    async fn resolve_event_offset(
        State(ledger): State<T>,
        path_values: PathMap,
        Extension(tx_number): Extension<TxNumber>,
        mut request: Request,
        next: Next,
    ) -> Result<Response, Response> {
        let event_offset = get_path_number(&path_values, "eventOffset")?;
        let identifier = EventIdentifier::TxIdAndOffset(TxIdAndOffset {
            tx_id: TxIdentifier::Number(tx_number.0),
            offset: event_offset,
        });
        let event_number = ledger
            .resolve_event_identifier(&identifier)
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Event", event_offset))?;

        request.extensions_mut().insert(EventNumber(event_number));
        Ok(next.run(request).await)
    }

    async fn get_latest_aggregated_proof(State(ledger): State<T>) -> ApiResult<AggregatedProof> {
        let latest_proof: AggregatedProof = ledger
            .get_latest_aggregated_proof()
            .await
            .map_err(database_error_response_500)?
            .ok_or_else(|| not_found_404("Aggregated proof", "latest"))?
            .try_into()
            .map_err(internal_server_error_response_500)?;

        Ok(latest_proof.into())
    }

    // SUBSCRIPTIONS
    // -------------

    async fn internal_generic_subscribe<S, M>(mut socket: WebSocket, mut subscription: S)
    where
        S: futures::Stream<Item = anyhow::Result<M>> + Unpin,
        M: Clone + Serialize + Send + Sync + 'static,
    {
        loop {
            tokio::select! {
                msg = socket.recv() => {
                    match msg {
                        Some(Err(error)) => {
                            warn!(?error, "Websocket error");
                            return;
                        },
                        None => {
                            // The client disconnected.
                            return;
                        },
                        Some(Ok(_)) => {
                            // Ignore incoming messages.
                        },
                    }
                },
                data_res = subscription.next() => {
                    match data_res {
                        Some(Ok(data)) => {
                            let Ok(serialized) = serde_json::to_string(&data) else {
                                return
                            };
                            let message = ws::Message::Text(serialized);
                            if let Err(err) = socket.send(message).await {
                                warn!(?err, "Websocket error while sending data");
                                // Keep the loop going.
                            }
                        },
                        Some(Err(err)) => {
                            warn!(?err, "Webocket error while receiving data from internal Tokio channel");
                            return;
                        },
                        None => {
                            // No more data to send.
                            return;
                        },
                    }
                }
            }
        }
    }

    async fn subscribe_to_aggregated_proofs(
        State(ledger): State<T>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| async move {
            let subscription = BroadcastStream::new(ledger.subscribe_proof_saved()).map(|data| {
                data.context("Failed to subscribe to proofs")
                    .and_then(|data| {
                        AggregatedProof::try_from(data)
                            .context("Failed to convert proof to REST API representation")
                    })
            });
            Self::internal_generic_subscribe(socket, subscription).await;
        })
    }

    async fn subscribe_to_head(State(ledger): State<T>, ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(|socket| async move {
            let subscription = BroadcastStream::new(ledger.subscribe_slots())
                .then(|slot_num_res| async {
                    let slot_num = slot_num_res?;
                    let Ok(Some(slot)) = ledger
                        .get_slot_by_number::<B, TxReceipt>(slot_num, QueryMode::Compact)
                        .await
                    else {
                        anyhow::bail!("Slot with number {} does not exist", slot_num);
                    };
                    Ok(Slot::<B, TxReceipt, E>::new(slot))
                })
                .boxed();

            Self::internal_generic_subscribe(socket, subscription).await;
        })
    }

    async fn subscribe_to_finalized(
        State(ledger): State<T>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(|socket| async move {
            let Ok(last_notified_slot) = ledger.get_latest_finalized_slot_number().await else {
                return;
            };

            let subscription = WatchStream::new(ledger.subscribe_finalized_slots())
                .zip(futures::stream::repeat((ledger, last_notified_slot)))
                .then(move |(slot_num, (ledger, last_notified_slot))| async move {
                    let mut slots = vec![];
                    for slot_number in last_notified_slot..=slot_num {
                        let slot_result = match ledger
                            .get_slot_by_number::<B, TxReceipt>(slot_number, QueryMode::Compact)
                            .await
                        {
                            Ok(Some(slot)) => Ok(slot),
                            Ok(None) => Err(anyhow::anyhow!(
                                "Slot with number {} does not exist",
                                slot_number
                            )),
                            Err(err) => Err(anyhow::anyhow!(
                                "Failed to query slot with number: {}",
                                err.to_string()
                            )),
                        };

                        slots.push(slot_result);
                    }

                    (slot_num, futures::stream::iter(slots))
                })
                .map(|tuple| tuple.1)
                .flatten()
                .boxed();

            Self::internal_generic_subscribe(socket, subscription).await;
        })
    }
}

#[derive(Deserialize)]
struct EventFilter {
    prefix: String,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct IncludeChildren {
    children: u8,
}

impl IncludeChildren {
    fn includes_children(&self) -> bool {
        self.children != 0
    }
}

impl From<IncludeChildren> for QueryMode {
    fn from(value: IncludeChildren) -> Self {
        if value.includes_children() {
            QueryMode::Full
        } else {
            QueryMode::Compact
        }
    }
}

#[serde_with::serde_as]
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum NumberOrHash {
    Number(#[serde_as(as = "serde_with::DisplayFromStr")] u64),
    Hash(HexHash),
}

impl NumberOrHash {
    fn as_u64(&self) -> Option<u64> {
        match self {
            NumberOrHash::Number(number) => Some(*number),
            _ => None,
        }
    }
}

impl Display for NumberOrHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NumberOrHash::Number(number) => number.fmt(f),
            NumberOrHash::Hash(hash) => hash.fmt(f),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename = "slot",
    rename_all = "camelCase",
    bound = "B: Serialize + DeserializeOwned, TxReceipt: TxReceiptContents, E: Serialize + DeserializeOwned"
)]
struct Slot<B, TxReceipt: TxReceiptContents, E> {
    pub number: u64,
    pub hash: HexHash,
    pub state_root: HexString,
    pub batch_range: Range<u64>,
    pub batches: Vec<Batch<B, TxReceipt, E>>,
    pub finality_status: FinalityStatus,
}

impl<B, TxReceipt: TxReceiptContents, E> Slot<B, TxReceipt, E> {
    fn new(slot: SlotResponse<B, TxReceipt>) -> Self {
        let mut batches = vec![];

        for batch_response in slot.batches.unwrap_or_default().into_iter() {
            if let ItemOrHash::Full(batch) = batch_response {
                batches.push(Batch::new(batch, slot.number));
            }
        }

        Self {
            number: slot.number,
            hash: HexHash::new(slot.hash),
            state_root: HexString(slot.state_root),
            batch_range: slot.batch_range,
            batches,
            finality_status: slot.finality_status,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename = "batch",
    rename_all = "camelCase",
    bound = "B: Serialize + DeserializeOwned, TxReceipt: TxReceiptContents, E: Serialize + DeserializeOwned"
)]
struct Batch<B, TxReceipt: TxReceiptContents, E> {
    pub number: u64,
    pub hash: HexHash,
    pub tx_range: Range<u64>,
    pub receipt: B,
    pub txs: Vec<Transaction<TxReceipt, E>>,
}

impl<B, TxReceipt: TxReceiptContents, E> Batch<B, TxReceipt, E> {
    fn new(batch: BatchResponse<B, TxReceipt>, number: u64) -> Self {
        let mut txs = vec![];

        for tx_response in batch.txs.unwrap_or_default().into_iter() {
            if let ItemOrHash::Full(tx) = tx_response {
                txs.push(Transaction::new(tx, number));
            }
        }

        Self {
            number,
            hash: HexHash::new(batch.hash),
            tx_range: batch.tx_range,
            receipt: batch.receipt,
            txs,
        }
    }
}

#[serde_as]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename = "tx",
    rename_all = "camelCase",
    bound = "TxReceipt: TxReceiptContents, E: Serialize + DeserializeOwned"
)]
struct Transaction<TxReceipt: TxReceiptContents, E> {
    pub number: u64,
    pub hash: HexHash,
    pub event_range: Range<u64>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub body: Vec<u8>,
    pub receipt: TxEffect<TxReceipt>,
    pub events: Vec<Event<E>>,
}

impl<TxReceipt: TxReceiptContents, E> Transaction<TxReceipt, E> {
    fn new(tx: TxResponse<TxReceipt>, number: u64) -> Self {
        Self {
            number,
            hash: HexHash::new(tx.hash),
            event_range: tx.event_range,
            body: tx.body.unwrap_or_default(),
            receipt: tx.receipt.into(),
            events: vec![],
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "event", rename_all = "camelCase")]
struct Event<E> {
    pub number: u64,
    pub key: String,
    pub value: E,
    pub module: ModuleRef,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum TxEffect<T: TxReceiptContents> {
    Skipped { data: T::Skipped },
    Reverted { data: T::Reverted },
    Successful { data: T::Successful },
}

impl<T: TxReceiptContents> From<sov_rollup_interface::stf::TxEffect<T>> for TxEffect<T> {
    fn from(value: sov_rollup_interface::stf::TxEffect<T>) -> Self {
        match value {
            sov_rollup_interface::stf::TxEffect::Skipped(data) => TxEffect::Skipped { data },
            sov_rollup_interface::stf::TxEffect::Reverted(data) => TxEffect::Reverted { data },
            sov_rollup_interface::stf::TxEffect::Successful(data) => TxEffect::Successful { data },
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "moduleRef", rename_all = "camelCase")]
struct ModuleRef {
    pub name: String,
}

// This type supplies the JSON API representation of [`AggregatedProofResponse`].
#[serde_as]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename = "aggregatedProof", rename_all = "camelCase")]
struct AggregatedProof {
    #[serde_as(as = "serde_with::base64::Base64")]
    pub proof: Vec<u8>,
    pub public_data: AggregatedProofPublicData,
}

impl TryFrom<AggregatedProofResponse> for AggregatedProof {
    type Error = anyhow::Error;

    fn try_from(value: AggregatedProofResponse) -> Result<Self, Self::Error> {
        let proof: Vec<u8> = value.proof.serialized_proof().to_vec();
        let data = value.proof.public_data();

        let public_data = AggregatedProofPublicData {
            validity_conditions: data
                .validity_conditions
                .iter()
                .map(|v| ValidityCondition(v.clone()))
                .collect(),

            rewarded_addresses: data
                .rewarded_addresses
                .iter()
                .map(|v| RewardedAddresses(v.clone()))
                .collect(),

            initial_slot_number: data.initial_slot_number,
            final_slot_number: data.final_slot_number,
            genesis_state_root: data.genesis_state_root.clone(),
            initial_state_root: data.initial_state_root.clone(),
            final_state_root: data.final_state_root.clone(),
            initial_slot_hash: data.initial_slot_hash.clone(),
            final_slot_hash: data.final_slot_hash.clone(),
            code_commitment: data.code_commitment.0.clone(),
        };

        Ok(Self { proof, public_data })
    }
}

#[serde_as]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidityCondition(#[serde_as(as = "serde_with::base64::Base64")] Vec<u8>);

#[serde_as]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RewardedAddresses(#[serde_as(as = "serde_with::base64::Base64")] Vec<u8>);

#[serde_as]
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AggregatedProofPublicData {
    pub validity_conditions: Vec<ValidityCondition>,
    pub initial_slot_number: u64,
    pub final_slot_number: u64,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub genesis_state_root: Vec<u8>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub initial_state_root: Vec<u8>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub final_state_root: Vec<u8>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub initial_slot_hash: Vec<u8>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub final_slot_hash: Vec<u8>,
    #[serde_as(as = "serde_with::base64::Base64")]
    pub code_commitment: Vec<u8>,
    pub rewarded_addresses: Vec<RewardedAddresses>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_spec_is_valid() {
        let _spec = openapi_spec();
    }
}
