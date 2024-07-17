use std::any::Any;

/// Represents a convenience struct to track the event and its type, functioning similarly to a typemap.
///
/// This struct is used to store information about an event, including its key, type identifier,
/// and the event itself encapsulated in a boxed trait object.
///
/// # Fields
/// - `event_key`: A vector of bytes representinexamples/simple-nft-module/README.mdg the unique key of the event.
/// - `type_id`: The type identifier of the event, using [`core::any::TypeId`].
/// - `boxed_event`: The event encapsulated in a box, implementing [`core::any::Any`] and [`core::marker::Send`].
#[derive(Debug)]
pub struct TypedEvent {
    event_key: Vec<u8>,
    type_id: core::any::TypeId,
    boxed_event: Box<dyn core::any::Any + core::marker::Send>,
}

impl TypedEvent {
    /// Created a Typed Event
    pub fn new<E: 'static + core::marker::Send>(event_key: &str, event: E) -> Self {
        TypedEvent {
            event_key: event_key.as_bytes().to_vec(),
            type_id: event.type_id(),
            boxed_event: Box::new(event),
        }
    }

    /// Try to cast from the TypedEvent to a specific type E provided
    /// checks type_id to avoid un-necessary casting
    pub fn downcast<E: core::clone::Clone + 'static>(self) -> Option<E> {
        if core::any::TypeId::of::<E>() == self.type_id {
            self.boxed_event.downcast::<E>().ok().map(|boxed| *boxed)
        } else {
            None
        }
    }

    // Try to cast from the TypedEvent to a specific type E provided
    /// checks type_id to avoid un-necessary casting
    pub fn downcast_ref<E: core::clone::Clone + 'static>(&self) -> Option<&E> {
        if core::any::TypeId::of::<E>() == self.type_id {
            self.boxed_event.downcast_ref::<E>()
        } else {
            None
        }
    }

    /// Function to peek at the type id
    pub fn type_id(&self) -> &core::any::TypeId {
        &self.type_id
    }

    /// Function to peek at the event key
    pub fn event_key(&self) -> &[u8] {
        &self.event_key
    }
}
