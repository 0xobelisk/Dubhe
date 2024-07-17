//! This module defines a codec which delegates to one codec for keys and one codec for values.

use super::StateCodec;

/// A [`StateCodec`] that uses one pre-existing codec for keys and a different one values.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct SplitCodec<KC, VC> {
    /// The codec to use for keys.
    pub key_codec: KC,
    /// The codec to use for values.
    pub value_codec: VC,
}

impl<KC, VC> StateCodec for SplitCodec<KC, VC>
where
    KC: Default + Clone + Send + Sync + 'static,
    VC: Default + Clone + Send + Sync + 'static,
{
    type KeyCodec = KC;
    type ValueCodec = VC;

    fn key_codec(&self) -> &Self::KeyCodec {
        &self.key_codec
    }

    fn value_codec(&self) -> &Self::ValueCodec {
        &self.value_codec
    }
}
