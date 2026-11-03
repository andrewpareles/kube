use std::collections::HashMap;

use bincode::Options;
use game_types::object_types::{
    ClockInfo, ClockState, InterpolationStyle, MaterialInfo, ObjectBeingAdded, ObjectLocation, PropertyAnimationInfo, PropertyAnimatorInfo,
    PropertyKeyframeInfo, WeightTransition, material::AlphaMode,
};
use serde_json::json;
use shared::protocol::{
    bincode_config::get_bincode_config,
    messages::{ObjectAddDelta, ObjectDelta, ServerMessageUDP, ServerSnapshotMessage},
};

#[test]
fn clock_state_json_uses_normal_serde_enum_shape() -> anyhow::Result<()> {
    let clock_state = ClockState::Playing {
        localStartMs: 123.0,
        timeLocation: ObjectLocation::Server,
    };

    let clock_state_json = serde_json::to_value(clock_state)?;

    assert_eq!(
        clock_state_json,
        json!({
            "Playing": {
                "localStartMs": 123.0,
                "timeLocation": "Server"
            }
        })
    );
    Ok(())
}

#[test]
fn weight_transition_json_uses_flat_shape() -> anyhow::Result<()> {
    let weight_transition = WeightTransition {
        startedAtMs: 100.0,
        timeLocation: ObjectLocation::Server,
        initValue: 10.0,
        finalValue: 50.0,
        durationMs: 250.0,
        easing: InterpolationStyle::EaseOut,
    };

    let weight_transition_json = serde_json::to_value(weight_transition)?;

    assert_eq!(
        weight_transition_json,
        json!({
            "startedAtMs": 100.0,
            "timeLocation": "Server",
            "initValue": 10.0,
            "finalValue": 50.0,
            "durationMs": 250.0,
            "easing": "EaseOut"
        })
    );
    Ok(())
}

#[test]
fn server_udp_snapshot_with_object_state_enums_round_trips_through_bincode() -> anyhow::Result<()> {
    let mut delta_of_object_id = HashMap::new();
    delta_of_object_id.insert(
        1,
        ObjectDelta::Add(ObjectAddDelta {
            name: "Clock".to_string(),
            objectBeingAdded: ObjectBeingAdded::Clock(ClockInfo {
                clockState: ClockState::Playing {
                    localStartMs: 100.0,
                    timeLocation: ObjectLocation::Server,
                },
                shouldLoop: true,
                speed: 1.0,
            }),
            parentId: 0,
            initSubitemRelativeLocation3D: None,
            teleportCount: 0,
        }),
    );
    delta_of_object_id.insert(
        3,
        ObjectDelta::Add(ObjectAddDelta {
            name: "PropertyAnimation".to_string(),
            objectBeingAdded: ObjectBeingAdded::PropertyAnimation(PropertyAnimationInfo {
                duration: 1.0,
                interpolationStyle: InterpolationStyle::Linear,
            }),
            parentId: 0,
            initSubitemRelativeLocation3D: None,
            teleportCount: 0,
        }),
    );
    delta_of_object_id.insert(
        4,
        ObjectDelta::Add(ObjectAddDelta {
            name: "PropertyKeyframe".to_string(),
            objectBeingAdded: ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo { time: 0.5, value: 3.25 }),
            parentId: 3,
            initSubitemRelativeLocation3D: None,
            teleportCount: 0,
        }),
    );
    delta_of_object_id.insert(
        2,
        ObjectDelta::Add(ObjectAddDelta {
            name: "PropertyAnimator".to_string(),
            objectBeingAdded: ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
                targetObjectId: None,
                propertyPath: vec!["weight".to_string()],
                propertyAnimationId: None,
                clockId: None,
                priority: 5,
                weight: 100.0,
                weightTransition: Some(WeightTransition {
                    startedAtMs: 100.0,
                    timeLocation: ObjectLocation::Server,
                    initValue: 0.0,
                    finalValue: 100.0,
                    durationMs: 250.0,
                    easing: InterpolationStyle::EaseOut,
                }),
            }),
            parentId: 0,
            initSubitemRelativeLocation3D: None,
            teleportCount: 0,
        }),
    );
    delta_of_object_id.insert(
        5,
        ObjectDelta::Add(ObjectAddDelta {
            name: "Blend material".to_string(),
            objectBeingAdded: ObjectBeingAdded::Material(MaterialInfo {
                albedoTextureDataId: Some(20),
                metallicTextureDataId: None,
                roughnessTextureDataId: None,
                normalTextureDataId: None,
                alphaMode: AlphaMode::Blend,
                isDoubleSided: true,
            }),
            parentId: 0,
            initSubitemRelativeLocation3D: None,
            teleportCount: 0,
        }),
    );
    let message = ServerMessageUDP::Snapshot(ServerSnapshotMessage {
        message_num: 1,
        live_server_sim_ms: 100.0,
        deltaOfObjectId: delta_of_object_id,
        deltaOfObjectId2: HashMap::new(),
    });

    let bincode_config = get_bincode_config();
    let bytes = bincode_config.serialize(&message)?;
    let parsed_message = bincode_config.deserialize::<ServerMessageUDP>(&bytes)?;

    let ServerMessageUDP::Snapshot(snapshot) = parsed_message else {
        panic!("Expected snapshot message");
    };
    assert_eq!(snapshot.deltaOfObjectId.len(), 5);
    let ObjectDelta::Add(material_delta) = &snapshot.deltaOfObjectId[&5] else {
        panic!("Expected material addition")
    };
    let ObjectBeingAdded::Material(material) = &material_delta.objectBeingAdded else {
        panic!("Expected material")
    };
    assert_eq!(material.albedoTextureDataId, Some(20));
    assert!(material.isDoubleSided);
    assert!(match material.alphaMode {
        AlphaMode::Blend => true,
        _ => false,
    });
    Ok(())
}
