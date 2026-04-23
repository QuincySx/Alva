// INPUT:  communication module
// OUTPUT: SpawnCommunication, SpawnCommContext, SpawnCommHandle, SpawnCommError, OnChildComplete, SpawnResult, SpawnCommunicationRegistry
// POS:    Sub-module grouping spawn-time communication trait vocabulary (plugin contract for attaching comm capabilities to child agents).

mod communication;

pub use communication::*;
