use crate::object_types::shared::ObjectIdPointer;
use crate::data_types::DataId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoundPlayerInfo {
    pub soundDataId: DataId,
    pub volume: f32,
    pub clockId: Option<ObjectIdPointer>,
}

pub struct SoundPlayerInfoTrackedFields {
    pub clockId: Option<ObjectIdPointer>,
}
