use serde::{Deserialize, Serialize};

// Scripts
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ObjectLocation {
    Client,
    Server,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub objectLocation: ObjectLocation,
}
