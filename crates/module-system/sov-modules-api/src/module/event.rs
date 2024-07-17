/// A trait that enables event processing for storage
pub trait RuntimeEventProcessor {
    /// Type specifying the wrapped enum for all events in the runtime
    type RuntimeEvent: borsh::BorshDeserialize
        + borsh::BorshSerialize
        + serde::Serialize
        + serde::de::DeserializeOwned
        + core::fmt::Debug
        + Clone
        + PartialEq
        + Send
        + Sync
        + EventModuleName;

    /// Function that converts module specific events to a wrapped event for storage
    fn convert_to_runtime_event(event: crate::TypedEvent) -> Option<Self::RuntimeEvent>;
}

/// Trait to get the module name from a specific runtime event.
pub trait EventModuleName {
    /// Returns the name of the module that emitted this event.
    fn module_name(&self) -> &'static str;
}

/// The response type for a module specific event
#[derive(
    Debug,
    PartialEq,
    Clone,
    borsh::BorshSerialize,
    borsh::BorshDeserialize,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(bound = "")]
pub struct RuntimeEventResponse<E>
where
    E: EventModuleName
        + Clone
        + borsh::BorshDeserialize
        + borsh::BorshSerialize
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    pub event_number: u64,
    /// Event key that was emitted along with this event
    pub event_key: String,
    /// A value representing the module event
    pub event_value: E,
    /// Module name
    pub module_name: String,
}

/// TryFrom trait implementation to create a RuntimeEventResponse for Stored Event
impl<E> TryFrom<(u64, sov_rollup_interface::stf::StoredEvent)> for RuntimeEventResponse<E>
where
    E: EventModuleName
        + Clone
        + borsh::BorshDeserialize
        + borsh::BorshSerialize
        + serde::Serialize
        + serde::de::DeserializeOwned,
{
    type Error = anyhow::Error;

    fn try_from(
        (event_number, stored_event): (u64, sov_rollup_interface::stf::StoredEvent),
    ) -> Result<Self, Self::Error> {
        let runtime_event: E =
            borsh::de::BorshDeserialize::try_from_slice(stored_event.value().inner().as_slice())
                .map_err(anyhow::Error::from)?;

        let key_str = String::from_utf8(stored_event.key().inner().clone())
            .unwrap_or_else(|_| hex::encode(stored_event.key().inner()));

        let module_name = runtime_event.module_name().to_string();

        Ok(Self {
            event_number,
            event_key: key_str,
            event_value: runtime_event,
            module_name,
        })
    }
}
