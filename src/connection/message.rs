use crate::connection::{ConnectionError, ConnectionResult};
use serde::{Deserialize, Serialize};

pub trait Message: Send + Sync {
    fn serialize(&self) -> ConnectionResult<Vec<u8>>;
}

pub trait DeserializeMessage: Sized {
    fn deserialize(data: &[u8]) -> ConnectionResult<Self>;
}

// Implementation for types that implement Serialize/Deserialize
impl<T: Serialize + Send + Sync> Message for T {
    fn serialize(&self) -> ConnectionResult<Vec<u8>> {
        bincode::serialize(self).map_err(|e| ConnectionError::SerializationError(e.to_string()))
    }
}

impl<T: for<'de> Deserialize<'de>> DeserializeMessage for T {
    fn deserialize(data: &[u8]) -> ConnectionResult<Self> {
        bincode::deserialize(data).map_err(|e| ConnectionError::SerializationError(e.to_string()))
    }
}
