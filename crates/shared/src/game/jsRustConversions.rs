use game_types::{
    consts::{ANIMATOR_PRIORITY_MAX, ANIMATOR_WEIGHT_MAX},
    data_types::DataId,
    object_types::{
        BallInfo, BallMaterialSettings, BillboardMode, BrickInfo, BrickMaterialSettings, ClockInfo, ClockState, CylinderInfo, CylinderMaterialSettings,
        InterpolationStyle, LabelHorizontalAlign, LabelImageInfo, LabelInfo, LocalGroupInfo, Location3D, MaterialInfo, MeshInfo, MeshMaterialSettings, Movement3D,
        ObjectBeingAdded, ObjectId, ObjectIdPointer, ObjectInfo, ObjectInfoTrackedFields, ObjectLocation, Orientation3D, PhysicsInfo3D, PhysicsToggles3D,
        PivotInfo, PlayerInfo, RigidGroupInfo, ScriptInfo, SkAnimationInfo, SkAttachmentInfo, SkJointInfo, SkKeyframeInfo, SkKeyframePoseInfo,
        SkeletonInfo, SoundPlayerInfo, TweenInfo, WeightTransition, WorldLightingInfo,
        material::AlphaMode,
        property_animation::PropertyAnimationInfo,
        property_animator::PropertyAnimatorInfo,
        property_keyframe::PropertyKeyframeInfo,
        shared::{Color3, VisualSettings, euler_degrees_to_quat, quat_to_euler_degrees},
        sk_animator::SkAnimatorInfo,
    },
    physics_primitive_types::{BallPrimitiveSettings, BrickPrimitiveSettings, CylinderPrimitiveSettings, PrimitiveSettings},
};
use crate::conversions::float_conversions::ToF64;
use crate::game::physics::{SetOrForce, SetPhysicsInfo3D, SetPhysicsToggles3D};
use crate::game::world::World;
use crate::protocol::messages::{ObjectModifyFlags, someIffTrue};
use deno_core::v8;
use glam::{Quat, Vec3};
use serde_json::{Map, Number, Value}; // inner working types, different from v8::Value
use v8::{Local, PinScope};

// ---------------- Note ----------------
/*

Inputs to:
rustUpdate<NAME>Field - Sometimes a Value (if the object's fields are all primitives), sometimes a ValueWithPath (otherwise)
rustSet<NAME> - always Value
rustUpdate<NAME> - always ValueWithPath
rust<NAME>Info - always Value

Notes:
- Functions with "Set" in the name like to call other Set functions, and with "Update" like Update
- js<NAME> always produces the v8::{Value|Object|etc} type
- rust<NAME> converts js to rust. We always convert the js value to serde's JSON "Value" type initally so we can manipulate it more easily than v8 Value.
- json<NAME> and js<NAME> must always match in logic 100%, and should always be implemented right on lines right above and below each other.
    - json<NAME> must always be implemented exactly as js<NAME> is
    - there should be an exact match for json<NAME> and js<NAME>. No extra json<NAME> functions and no extra js<NAME> functions.

TODOs:
- optimize when nextFieldName, nextValue, expectFinalNextValue are called at the same time (some redundant computations)
- Simplify user-facing error messages
*/

// ---------------- ValueWithPath: Helper for updating Rust from JS ----------------

pub struct ValueWithPath<'v, 'p> {
    value: &'v Value,
    fullRemainingPath: &'p [String],
}
impl<'v, 'p> ValueWithPath<'v, 'p> {
    pub fn new(value: &'v Value, fullRemainingPath: &'p [String]) -> Self {
        ValueWithPath { value, fullRemainingPath }
    }

    fn expect_path_head<'a>(path: &'a [String], error_from: &'static str) -> anyhow::Result<(&'a String, &'a [String])> {
        path.split_first().ok_or_else(|| anyhow::anyhow!("{error_from}: expected string"))
    }

    fn nextFieldName(self: &Self, error_from: &'static str) -> anyhow::Result<&'p String> {
        let head = Self::expect_path_head(self.fullRemainingPath, error_from)?;
        let (field, _) = head;
        Ok(field)
    }

    fn nextValue(self: &Self) -> anyhow::Result<Self> {
        let (_, rest) = Self::expect_path_head(self.fullRemainingPath, "object_indexing")?;
        Ok(ValueWithPath {
            value: self.value,
            fullRemainingPath: rest,
        })
    }

    fn expectFinalNextValue(self: &Self) -> anyhow::Result<&'v Value> {
        let next = self.nextValue()?;
        if !next.fullRemainingPath.is_empty() {
            return Err(anyhow::anyhow!("object_indexing: Didn't expect any further indexing."));
        }
        Ok(next.value)
    }
}

// {a: {b: value}} for the path [a, b]. Only enums need it: writing inside one variant, like clockState.Paused, parses the
// whole tagged variant.
fn jsonAlongPath(path: &[String], value: &Value) -> Value {
    path.iter().rev().fold(value.clone(), |inner, field| Value::Object(Map::from_iter([(field.clone(), inner)])))
}

// ---------------- JS Helpers ----------------

// These helpers build fresh return values while the caller may still borrow the world.
// Define own properties instead of assigning through prototypes, which could run user setters.
fn jsSetField<'s, 'i, F, V>(obj: Local<v8::Object>, key: &str, f: F, scope: &mut PinScope<'s, 'i>) -> anyhow::Result<()>
where
    F: FnOnce(&mut PinScope<'s, 'i>) -> anyhow::Result<V>,
    V: Into<Local<'s, v8::Value>>,
{
    let val: Local<'s, v8::Value> = f(scope)?.into();
    let k = jsString(key, scope)?;
    if obj.create_data_property(scope, k.into(), val) != Some(true) {
        return Err(anyhow::anyhow!("Could not define JS field {key}"));
    }
    Ok(())
}

// JSON value -> JSON object "map"
pub fn asJSON<'a>(jsvalue: &'a Value, ctx: &'static str) -> anyhow::Result<&'a Map<String, Value>> {
    jsvalue.as_object().ok_or_else(|| anyhow::anyhow!("{ctx}: expected object"))
}

// read json map field
fn readJSON<'a>(object: &'a Map<String, Value>, key: &str, ctx: &'static str) -> anyhow::Result<&'a Value> {
    object.get(key).ok_or_else(|| anyhow::anyhow!("{ctx}: missing field {key}"))
}

fn jsDataId<'s>(id: DataId, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Number> {
    jsNumber(id, scope)
}

pub fn jsObjectId(id: ObjectId) -> i64 {
    id as i64
}

fn jsOptionalObjectId<'s>(id: Option<ObjectId>, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Value> {
    jsNumberOrNull(id, scope)
}

fn jsOptionalDataId<'s>(id: Option<DataId>, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Value> {
    jsNumberOrNull(id, scope)
}

// ------------ Primitives for updating Rust from JS ------------

fn rustf32(jsvalue: &Value) -> anyhow::Result<f32> {
    let n = match jsvalue {
        Value::Number(n) => n,
        _ => return Err(anyhow::anyhow!("value: number")),
    };
    let v = n.as_f64().ok_or_else(|| anyhow::anyhow!("value: number"))? as f32;
    // remove all NaN and infinities
    if !v.is_finite() {
        return Err(anyhow::anyhow!("value: finite number"));
    }
    Ok(v)
}

fn rustf64(jsvalue: &Value) -> anyhow::Result<f64> {
    let v = jsvalue.as_f64().ok_or_else(|| anyhow::anyhow!("value: number"))?;
    // remove all NaN and infinities
    if !v.is_finite() {
        return Err(anyhow::anyhow!("value: finite number"));
    }
    Ok(v)
}

fn rustRadiansFromDegrees(jsvalue: &Value) -> anyhow::Result<f32> {
    Ok(rustf32(jsvalue)?.to_radians())
}

fn rustU8(jsvalue: &Value) -> anyhow::Result<u8> {
    let f32 = rustf32(jsvalue)?;
    let clamp = f32.clamp(0.0, u8::MAX as f32);
    let u8 = clamp as u8; // uses trunc()
    Ok(u8)
}

fn rustAnimationWeight(jsvalue: &Value) -> anyhow::Result<f32> {
    let weight = rustf32(jsvalue)?;
    Ok(weight.clamp(0.0, ANIMATOR_WEIGHT_MAX))
}

fn rustAnimatorPriority(jsvalue: &Value) -> anyhow::Result<u8> {
    let priority = rustU8(jsvalue)?;
    Ok(priority.clamp(0, ANIMATOR_PRIORITY_MAX))
}

fn rustBool(jsvalue: &Value) -> anyhow::Result<bool> {
    jsvalue.as_bool().ok_or_else(|| anyhow::anyhow!("value: bool"))
}

fn rustString(jsvalue: &Value) -> anyhow::Result<String> {
    jsvalue.as_str().map(|s| s.to_string()).ok_or_else(|| anyhow::anyhow!("value: string"))
}

fn rustObjectId(jsvalue: &Value) -> anyhow::Result<ObjectId> {
    let id: i64 = jsvalue.as_i64().ok_or_else(|| anyhow::anyhow!("value: number"))?;
    let id = id as ObjectId;
    Ok(id)
}

fn rustOptionObjectId(jsvalue: &Value) -> anyhow::Result<Option<ObjectId>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => {
            let id = rustObjectId(jsvalue)?;
            Ok(Some(id))
        }
    }
}

fn rustDataId(jsvalue: &Value) -> anyhow::Result<DataId> {
    let id: i64 = jsvalue.as_i64().ok_or_else(|| anyhow::anyhow!("value: number"))?;
    let id = id as DataId;
    Ok(id)
}

fn rustOptionDataId(jsvalue: &Value) -> anyhow::Result<Option<DataId>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => {
            let id = rustDataId(jsvalue)?;
            Ok(Some(id))
        }
    }
}

fn rustOptioni64(jsvalue: &Value) -> anyhow::Result<Option<i64>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(jsvalue.as_i64().ok_or_else(|| anyhow::anyhow!("value: number"))?)),
    }
}

// ---------------- V8 Helpers ----------------

fn jsString<'s>(s: &str, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::String>> {
    v8::String::new(scope, s).ok_or_else(|| anyhow::anyhow!("Failed to create String. The program may be out of memory."))
}

pub fn jsonNumber(value: f32) -> anyhow::Result<Value> {
    let Some(number) = Number::from_f64(value as f64) else {
        return Err(anyhow::anyhow!("JSON number must be finite"));
    };
    Ok(Value::Number(number))
}

fn jsNumber<'s, N: ToF64>(n: N, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Number> {
    v8::Number::new(scope, n.to_f64())
}

fn jsDegrees<'s>(radians: f32, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Number> {
    jsNumber(radians.to_degrees(), scope)
}

fn jsBoolean<'s>(b: bool, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Boolean> {
    v8::Boolean::new(scope, b)
}

fn jsEmptyObject<'s>(scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Object> {
    v8::Object::new(scope)
}

fn jsNull<'s>(scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Primitive> {
    v8::null(scope)
}

fn jsNumberOrNull<'s, N: ToF64>(n: Option<N>, scope: &mut PinScope<'s, '_>) -> Local<'s, v8::Value> {
    match n {
        Some(value) => jsNumber(value, scope).into(),
        None => jsNull(scope).into(),
    }
}

fn jsStringArray<'s>(items: &[String], scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Array>> {
    let elements = items
        .iter()
        .map(|item| jsString(item, scope).map(Into::into))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(v8::Array::new_with_elements(scope, &elements))
}

fn rustStringArray(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<Vec<String>> {
    let array = jsvalue.as_array().ok_or_else(|| anyhow::anyhow!("{ctx}: expected array"))?;
    array.iter().map(rustString).collect()
}

// ---------------- Vec3 ----------------

pub fn jsVec3<'s>(v: Vec3, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "x", |s| Ok(jsNumber(v.x, s)), scope)?;
    jsSetField(obj, "y", |s| Ok(jsNumber(v.y, s)), scope)?;
    jsSetField(obj, "z", |s| Ok(jsNumber(v.z, s)), scope)?;
    Ok(obj)
}

pub fn rustVec3(jsvalue: &Value) -> anyhow::Result<Vec3> {
    let object = asJSON(jsvalue, "vec3")?;
    Ok(Vec3::new(
        rustf32(readJSON(object, "x", "vec3")?)?,
        rustf32(readJSON(object, "y", "vec3")?)?,
        rustf32(readJSON(object, "z", "vec3")?)?,
    ))
}

fn rustUpdateVec3Field(vec: &mut Vec3, axis: &str, jsvalue: &Value) -> anyhow::Result<()> {
    let rust_val = rustf32(jsvalue)?;
    match axis {
        "x" => vec.x = rust_val,
        "y" => vec.y = rust_val,
        "z" => vec.z = rust_val,
        _ => return Err(anyhow::anyhow!("axis: x|y|z")),
    }
    Ok(())
}
fn rustSetVec3(vec: &mut Vec3, jsvalue: &Value) -> anyhow::Result<()> {
    *vec = rustVec3(jsvalue)?;
    Ok(())
}
fn rustUpdateVec3<'v, 'p>(vec: &mut Vec3, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetVec3(vec, valueWithPath.value)
    } else {
        let field = valueWithPath.nextFieldName("update_vec3")?;
        let leaf_value = valueWithPath.expectFinalNextValue()?;
        rustUpdateVec3Field(vec, field.as_str(), leaf_value)
    }
}

fn jsVec3DegreesFromRadians<'s>(v: Vec3, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "x", |s| Ok(jsDegrees(v.x, s)), scope)?;
    jsSetField(obj, "y", |s| Ok(jsDegrees(v.y, s)), scope)?;
    jsSetField(obj, "z", |s| Ok(jsDegrees(v.z, s)), scope)?;
    Ok(obj)
}

pub fn rustVec3RadiansFromDegrees(jsvalue: &Value) -> anyhow::Result<Vec3> {
    let object = asJSON(jsvalue, "vec3_degrees")?;
    Ok(Vec3::new(
        rustRadiansFromDegrees(readJSON(object, "x", "vec3_degrees")?)?,
        rustRadiansFromDegrees(readJSON(object, "y", "vec3_degrees")?)?,
        rustRadiansFromDegrees(readJSON(object, "z", "vec3_degrees")?)?,
    ))
}

fn rustUpdateVec3RadiansFromDegreesField(vec: &mut Vec3, axis: &str, jsvalue: &Value) -> anyhow::Result<()> {
    let rust_val = rustRadiansFromDegrees(jsvalue)?;
    match axis {
        "x" => vec.x = rust_val,
        "y" => vec.y = rust_val,
        "z" => vec.z = rust_val,
        _ => return Err(anyhow::anyhow!("axis: x|y|z")),
    }
    Ok(())
}

fn rustSetVec3RadiansFromDegrees(vec: &mut Vec3, jsvalue: &Value) -> anyhow::Result<()> {
    *vec = rustVec3RadiansFromDegrees(jsvalue)?;
    Ok(())
}

fn rustUpdateVec3RadiansFromDegrees<'v, 'p>(vec: &mut Vec3, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetVec3RadiansFromDegrees(vec, valueWithPath.value)
    } else {
        let field = valueWithPath.nextFieldName("update_vec3_degrees")?;
        let leaf_value = valueWithPath.expectFinalNextValue()?;
        rustUpdateVec3RadiansFromDegreesField(vec, field.as_str(), leaf_value)
    }
}

// ---------------- Color3 ----------------

fn jsColor3<'s>(color3: &Color3, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "r", |s| Ok(jsNumber(color3.r, s)), scope)?;
    jsSetField(obj, "g", |s| Ok(jsNumber(color3.g, s)), scope)?;
    jsSetField(obj, "b", |s| Ok(jsNumber(color3.b, s)), scope)?;
    Ok(obj)
}

fn rustColor3(jsvalue: &Value) -> anyhow::Result<Color3> {
    let object = asJSON(jsvalue, "color3")?;
    Ok(Color3 {
        r: rustf32(readJSON(object, "r", "color3")?)?,
        g: rustf32(readJSON(object, "g", "color3")?)?,
        b: rustf32(readJSON(object, "b", "color3")?)?,
    })
}

fn rustUpdateColor3Field(color3: &mut Color3, channel: &str, jsvalue: &Value) -> anyhow::Result<()> {
    let rust_val = rustf32(jsvalue)?;
    match channel {
        "r" => color3.r = rust_val,
        "g" => color3.g = rust_val,
        "b" => color3.b = rust_val,
        _ => return Err(anyhow::anyhow!("color3: r|g|b")),
    }
    Ok(())
}
fn rustSetColor3(color3: &mut Color3, jsvalue: &Value) -> anyhow::Result<()> {
    *color3 = rustColor3(jsvalue)?;
    Ok(())
}
fn rustUpdateColor3<'v, 'p>(color3: &mut Color3, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetColor3(color3, valueWithPath.value)
    } else {
        let channel = valueWithPath.nextFieldName("update_color3")?;
        let leaf_value = valueWithPath.expectFinalNextValue()?;
        rustUpdateColor3Field(color3, channel.as_str(), leaf_value)
    }
}

// ---------------- ObjectLocation ----------------

fn jsObjectLocation<'s>(object_location: ObjectLocation, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Value>> {
    match object_location {
        ObjectLocation::Server => Ok(jsString("Server", scope)?.into()),
        ObjectLocation::Client => Ok(jsString("Client", scope)?.into()),
    }
}

fn rustObjectLocation(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<ObjectLocation> {
    let object_location = rustString(jsvalue)?;
    match object_location.as_str() {
        "Server" => Ok(ObjectLocation::Server),
        "Client" => Ok(ObjectLocation::Client),
        _ => Err(anyhow::anyhow!("{ctx}: Server|Client")),
    }
}

// ---------------- ClockState ----------------

fn jsClockState<'s>(clock_state: ClockState, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    match clock_state {
        ClockState::Paused { animationTimeSecs } => {
            jsSetField(
                obj,
                "Paused",
                |s| {
                    let paused = jsEmptyObject(s);
                    jsSetField(paused, "animationTimeSecs", |s| Ok(jsNumber(animationTimeSecs, s)), s)?;
                    Ok(paused)
                },
                scope,
            )?;
        }
        ClockState::Playing { localStartMs, timeLocation } => {
            jsSetField(
                obj,
                "Playing",
                |s| {
                    let playing = jsEmptyObject(s);
                    jsSetField(playing, "localStartMs", |s| Ok(jsNumber(localStartMs, s)), s)?;
                    jsSetField(playing, "timeLocation", |s| Ok(jsObjectLocation(timeLocation, s)?), s)?;
                    Ok(playing)
                },
                scope,
            )?;
        }
    }
    Ok(obj)
}

fn rustClockState(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<ClockState> {
    let object = asJSON(jsvalue, ctx)?;
    if object.len() != 1 {
        return Err(anyhow::anyhow!("{ctx}: ClockState must have exactly one variant"));
    }
    if let Some(paused_value) = object.get("Paused") {
        let paused = asJSON(paused_value, ctx)?;
        return ClockState::new_paused(rustf32(readJSON(paused, "animationTimeSecs", ctx)?)?);
    }
    if let Some(playing_value) = object.get("Playing") {
        let playing = asJSON(playing_value, ctx)?;
        return ClockState::new_playing(
            rustf64(readJSON(playing, "localStartMs", ctx)?)?,
            rustObjectLocation(readJSON(playing, "timeLocation", ctx)?, ctx)?,
        );
    }
    Err(anyhow::anyhow!("{ctx}: ClockState must have Paused or Playing"))
}

fn rustUpdateClockState<'v, 'p>(clock_state: &mut ClockState, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *clock_state = rustClockState(valueWithPath.value, "update_clockState")?;
        return Ok(());
    }

    let variant_value = jsonAlongPath(valueWithPath.fullRemainingPath, valueWithPath.value);
    let field = valueWithPath.nextFieldName("update_clockState")?;
    match field.as_str() {
        "Paused" => {
            let paused_value = valueWithPath.nextValue()?;
            if paused_value.fullRemainingPath.is_empty() {
                *clock_state = rustClockState(&variant_value, "update_clockState")?;
                return Ok(());
            }
            match clock_state {
                ClockState::Paused { .. } => {
                    let field = paused_value.nextFieldName("update_clockState.Paused")?;
                    match field.as_str() {
                        "animationTimeSecs" => *clock_state = ClockState::new_paused(rustf32(paused_value.expectFinalNextValue()?)?)?,
                        _ => return Err(anyhow::anyhow!("update_clockState.Paused: animationTimeSecs")),
                    }
                }
                ClockState::Playing { .. } => {
                    *clock_state = rustClockState(&variant_value, "update_clockState")?;
                }
            }
        }
        "Playing" => {
            let playing_value = valueWithPath.nextValue()?;
            if playing_value.fullRemainingPath.is_empty() {
                *clock_state = rustClockState(&variant_value, "update_clockState")?;
                return Ok(());
            }
            match clock_state {
                ClockState::Paused { .. } => {
                    *clock_state = rustClockState(&variant_value, "update_clockState")?;
                }
                ClockState::Playing { timeLocation, .. } => {
                    let field = playing_value.nextFieldName("update_clockState.Playing")?;
                    match field.as_str() {
                        "localStartMs" => *clock_state = ClockState::new_playing(rustf64(playing_value.expectFinalNextValue()?)?, *timeLocation)?,
                        "timeLocation" => *timeLocation = rustObjectLocation(playing_value.expectFinalNextValue()?, "update_clockState.Playing.timeLocation")?,
                        _ => return Err(anyhow::anyhow!("update_clockState.Playing: localStartMs|timeLocation")),
                    }
                }
            }
        }
        _ => return Err(anyhow::anyhow!("update_clockState: Paused|Playing")),
    }
    Ok(())
}

// ---------------- WeightTransition ----------------

fn jsInterpolationStyle<'s>(interpolationStyle: InterpolationStyle, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Value>> {
    Ok(jsString(interpolationStyle.asStr(), scope)?.into())
}

fn rustInterpolationStyle(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<InterpolationStyle> {
    let interpolationStyle = rustString(jsvalue)?;
    InterpolationStyle::fromString(&interpolationStyle).ok_or_else(|| anyhow::anyhow!("{ctx}: Step|Linear|EaseInOut|EaseIn|EaseOut"))
}

fn jsWeightTransition<'s>(weight_transition: &WeightTransition, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "startedAtMs", |s| Ok(jsNumber(weight_transition.startedAtMs, s)), scope)?;
    jsSetField(obj, "timeLocation", |s| Ok(jsObjectLocation(weight_transition.timeLocation, s)?), scope)?;
    jsSetField(obj, "initValue", |s| Ok(jsNumber(weight_transition.initValue, s)), scope)?;
    jsSetField(obj, "finalValue", |s| Ok(jsNumber(weight_transition.finalValue, s)), scope)?;
    jsSetField(obj, "durationMs", |s| Ok(jsNumber(weight_transition.durationMs, s)), scope)?;
    jsSetField(obj, "easing", |s| Ok(jsInterpolationStyle(weight_transition.easing, s)?), scope)?;
    Ok(obj)
}

fn jsOptionalWeightTransition<'s>(weight_transition: &Option<WeightTransition>, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Value>> {
    match weight_transition {
        Some(weight_transition) => Ok(jsWeightTransition(weight_transition, scope)?.into()),
        None => Ok(jsNull(scope).into()),
    }
}

fn rustWeightTransition(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<WeightTransition> {
    let object = asJSON(jsvalue, ctx)?;
    Ok(WeightTransition {
        startedAtMs: rustf64(readJSON(object, "startedAtMs", ctx)?)?,
        timeLocation: rustObjectLocation(readJSON(object, "timeLocation", ctx)?, ctx)?,
        initValue: rustAnimationWeight(readJSON(object, "initValue", ctx)?)?,
        finalValue: rustAnimationWeight(readJSON(object, "finalValue", ctx)?)?,
        durationMs: rustf32(readJSON(object, "durationMs", ctx)?)?,
        easing: rustInterpolationStyle(readJSON(object, "easing", ctx)?, ctx)?,
    })
}

fn rustOptionalWeightTransition(jsvalue: &Value, ctx: &'static str) -> anyhow::Result<Option<WeightTransition>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(rustWeightTransition(jsvalue, ctx)?)),
    }
}

fn rustUpdateOptionalWeightTransition<'v, 'p>(
    weight_transition: &mut Option<WeightTransition>,
    valueWithPath: ValueWithPath<'v, 'p>,
    ctx: &'static str,
) -> anyhow::Result<()> {
    if !valueWithPath.fullRemainingPath.is_empty() {
        return Err(anyhow::anyhow!("{ctx}: Didn't expect any further indexing."));
    }
    *weight_transition = rustOptionalWeightTransition(valueWithPath.value, ctx)?;
    Ok(())
}

// ---------------- AlphaMode ----------------

fn jsAlphaMode<'s>(alpha_mode: AlphaMode, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Value>> {
    match alpha_mode {
        AlphaMode::Opaque => Ok(jsString("Opaque", scope)?.into()),
        AlphaMode::Blend => Ok(jsString("Blend", scope)?.into()),
        AlphaMode::Mask { cutoff } => {
            let alpha_mode_object = jsEmptyObject(scope);
            let mask_object = jsEmptyObject(scope);
            jsSetField(mask_object, "cutoff", |s| Ok(jsNumber(cutoff, s)), scope)?;
            jsSetField(alpha_mode_object, "Mask", |_s| Ok(mask_object), scope)?;
            Ok(alpha_mode_object.into())
        }
    }
}

fn rustAlphaMode(jsvalue: &Value) -> anyhow::Result<AlphaMode> {
    match jsvalue {
        Value::String(alpha_mode_string) => match alpha_mode_string.as_str() {
            "Opaque" => Ok(AlphaMode::Opaque),
            "Blend" => Ok(AlphaMode::Blend),
            _ => Err(anyhow::anyhow!("alphaMode: Opaque|Mask|Blend")),
        },
        Value::Object(alpha_mode_object) => {
            let mask_value = readJSON(alpha_mode_object, "Mask", "alphaMode")?;
            let mask_object = asJSON(mask_value, "alphaMode.Mask")?;
            let cutoff = rustf32(readJSON(mask_object, "cutoff", "alphaMode.Mask")?)?;
            Ok(AlphaMode::Mask {
                cutoff: cutoff.clamp(0.0, 1.0),
            })
        }
        _ => Err(anyhow::anyhow!("alphaMode: Opaque|Mask|Blend")),
    }
}

fn rustUpdateAlphaMode<'v, 'p>(alpha_mode: &mut AlphaMode, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *alpha_mode = rustAlphaMode(valueWithPath.value)?;
        return Ok(());
    }

    let field = valueWithPath.nextFieldName("update_alphaMode")?;
    match field.as_str() {
        "Mask" => {
            if valueWithPath.fullRemainingPath.len() == 1 {
                *alpha_mode = rustAlphaMode(&jsonAlongPath(valueWithPath.fullRemainingPath, valueWithPath.value))?;
                return Ok(());
            }

            let AlphaMode::Mask { cutoff } = alpha_mode else {
                return Err(anyhow::anyhow!("update_alphaMode: set alphaMode to Mask before updating Mask.cutoff"));
            };
            let cutoff_value_with_path = valueWithPath.nextValue()?;
            let cutoff_field = cutoff_value_with_path.nextFieldName("update_alphaMode.Mask")?;
            match cutoff_field.as_str() {
                "cutoff" => {
                    *cutoff = rustf32(cutoff_value_with_path.expectFinalNextValue()?)?.clamp(0.0, 1.0);
                    Ok(())
                }
                _ => Err(anyhow::anyhow!("update_alphaMode.Mask: cutoff")),
            }
        }
        _ => Err(anyhow::anyhow!("update_alphaMode: Mask")),
    }
}

// ---------------- BallMaterialSettings ----------------

fn jsBallMaterialSettings<'s>(ballMaterialSettings: &BallMaterialSettings, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let object = jsEmptyObject(scope);
    jsSetField(
        object,
        "materialInfoId",
        |s| Ok(jsNumber(ObjectId::from(ballMaterialSettings.materialInfoId), s)),
        scope,
    )?;
    jsSetField(object, "tileSizeU", |s| Ok(jsNumber(ballMaterialSettings.tileSizeU, s)), scope)?;
    jsSetField(object, "tileSizeV", |s| Ok(jsNumber(ballMaterialSettings.tileSizeV, s)), scope)?;
    jsSetField(object, "tileOffsetU", |s| Ok(jsNumber(ballMaterialSettings.tileOffsetU, s)), scope)?;
    jsSetField(object, "tileOffsetV", |s| Ok(jsNumber(ballMaterialSettings.tileOffsetV, s)), scope)?;
    Ok(object)
}

fn jsOptionalBallMaterialSettings<'s>(
    ballMaterialSettings: &Option<BallMaterialSettings>,
    scope: &mut PinScope<'s, '_>,
) -> anyhow::Result<Local<'s, v8::Value>> {
    match ballMaterialSettings {
        Some(ballMaterialSettings) => Ok(jsBallMaterialSettings(ballMaterialSettings, scope)?.into()),
        None => Ok(jsNull(scope).into()),
    }
}

fn rustBallMaterialSettings(jsvalue: &Value) -> anyhow::Result<BallMaterialSettings> {
    let object = asJSON(jsvalue, "material")?;
    Ok(BallMaterialSettings {
        materialInfoId: ObjectIdPointer(rustObjectId(readJSON(object, "materialInfoId", "material")?)?),
        tileSizeU: rustf32(readJSON(object, "tileSizeU", "material")?)?,
        tileSizeV: rustf32(readJSON(object, "tileSizeV", "material")?)?,
        tileOffsetU: rustf32(readJSON(object, "tileOffsetU", "material")?)?,
        tileOffsetV: rustf32(readJSON(object, "tileOffsetV", "material")?)?,
    })
}

fn rustOptionalBallMaterialSettings(jsvalue: &Value) -> anyhow::Result<Option<BallMaterialSettings>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(rustBallMaterialSettings(jsvalue)?)),
    }
}

fn rustSetBallMaterialSettings(ballMaterialSettings: &mut BallMaterialSettings, jsvalue: &Value) -> anyhow::Result<()> {
    *ballMaterialSettings = rustBallMaterialSettings(jsvalue)?;
    Ok(())
}

fn rustUpdateBallMaterialSettingsField<'v, 'p>(
    ballMaterialSettings: &mut BallMaterialSettings,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    match field {
        "materialInfoId" => ballMaterialSettings.materialInfoId = ObjectIdPointer(rustObjectId(valueWithPath.expectFinalNextValue()?)?),
        "tileSizeU" => ballMaterialSettings.tileSizeU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileSizeV" => ballMaterialSettings.tileSizeV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetU" => ballMaterialSettings.tileOffsetU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetV" => ballMaterialSettings.tileOffsetV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        _ => {
            return Err(anyhow::anyhow!(
                "update_ball.material: materialInfoId|tileSizeU|tileSizeV|tileOffsetU|tileOffsetV"
            ));
        }
    }
    Ok(())
}

fn rustUpdateBallMaterialSettings<'v, 'p>(ballMaterialSettings: &mut BallMaterialSettings, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetBallMaterialSettings(ballMaterialSettings, valueWithPath.value)?;
        return Ok(());
    }

    let field = valueWithPath.nextFieldName("update_ball.material")?;
    rustUpdateBallMaterialSettingsField(ballMaterialSettings, field.as_str(), valueWithPath)
}

fn rustUpdateOptionalBallMaterialSettings<'v, 'p>(
    ballMaterialSettings: &mut Option<BallMaterialSettings>,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *ballMaterialSettings = rustOptionalBallMaterialSettings(valueWithPath.value)?;
        return Ok(());
    }

    let Some(ballMaterialSettings) = ballMaterialSettings.as_mut() else {
        return Err(anyhow::anyhow!("update_ball.material: set material before updating nested fields"));
    };
    rustUpdateBallMaterialSettings(ballMaterialSettings, valueWithPath)
}

// ---------------- CylinderMaterialSettings ----------------

fn jsCylinderMaterialSettings<'s>(cylinderMaterialSettings: &CylinderMaterialSettings, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let object = jsEmptyObject(scope);
    jsSetField(
        object,
        "materialInfoId",
        |s| Ok(jsNumber(ObjectId::from(cylinderMaterialSettings.materialInfoId), s)),
        scope,
    )?;
    jsSetField(object, "tileSizeU", |s| Ok(jsNumber(cylinderMaterialSettings.tileSizeU, s)), scope)?;
    jsSetField(object, "tileSizeV", |s| Ok(jsNumber(cylinderMaterialSettings.tileSizeV, s)), scope)?;
    jsSetField(object, "tileOffsetU", |s| Ok(jsNumber(cylinderMaterialSettings.tileOffsetU, s)), scope)?;
    jsSetField(object, "tileOffsetV", |s| Ok(jsNumber(cylinderMaterialSettings.tileOffsetV, s)), scope)?;
    Ok(object)
}

fn jsOptionalCylinderMaterialSettings<'s>(
    cylinderMaterialSettings: &Option<CylinderMaterialSettings>,
    scope: &mut PinScope<'s, '_>,
) -> anyhow::Result<Local<'s, v8::Value>> {
    match cylinderMaterialSettings {
        Some(cylinderMaterialSettings) => Ok(jsCylinderMaterialSettings(cylinderMaterialSettings, scope)?.into()),
        None => Ok(jsNull(scope).into()),
    }
}

fn rustCylinderMaterialSettings(jsvalue: &Value) -> anyhow::Result<CylinderMaterialSettings> {
    let object = asJSON(jsvalue, "material")?;
    Ok(CylinderMaterialSettings {
        materialInfoId: ObjectIdPointer(rustObjectId(readJSON(object, "materialInfoId", "material")?)?),
        tileSizeU: rustf32(readJSON(object, "tileSizeU", "material")?)?,
        tileSizeV: rustf32(readJSON(object, "tileSizeV", "material")?)?,
        tileOffsetU: rustf32(readJSON(object, "tileOffsetU", "material")?)?,
        tileOffsetV: rustf32(readJSON(object, "tileOffsetV", "material")?)?,
    })
}

fn rustOptionalCylinderMaterialSettings(jsvalue: &Value) -> anyhow::Result<Option<CylinderMaterialSettings>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(rustCylinderMaterialSettings(jsvalue)?)),
    }
}

fn rustSetCylinderMaterialSettings(cylinderMaterialSettings: &mut CylinderMaterialSettings, jsvalue: &Value) -> anyhow::Result<()> {
    *cylinderMaterialSettings = rustCylinderMaterialSettings(jsvalue)?;
    Ok(())
}

fn rustUpdateCylinderMaterialSettingsField<'v, 'p>(
    cylinderMaterialSettings: &mut CylinderMaterialSettings,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    match field {
        "materialInfoId" => cylinderMaterialSettings.materialInfoId = ObjectIdPointer(rustObjectId(valueWithPath.expectFinalNextValue()?)?),
        "tileSizeU" => cylinderMaterialSettings.tileSizeU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileSizeV" => cylinderMaterialSettings.tileSizeV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetU" => cylinderMaterialSettings.tileOffsetU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetV" => cylinderMaterialSettings.tileOffsetV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        _ => {
            return Err(anyhow::anyhow!(
                "update_cylinder.material: materialInfoId|tileSizeU|tileSizeV|tileOffsetU|tileOffsetV"
            ));
        }
    }
    Ok(())
}

fn rustUpdateCylinderMaterialSettings<'v, 'p>(
    cylinderMaterialSettings: &mut CylinderMaterialSettings,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetCylinderMaterialSettings(cylinderMaterialSettings, valueWithPath.value)?;
        return Ok(());
    }

    let field = valueWithPath.nextFieldName("update_cylinder.material")?;
    rustUpdateCylinderMaterialSettingsField(cylinderMaterialSettings, field.as_str(), valueWithPath)
}

fn rustUpdateOptionalCylinderMaterialSettings<'v, 'p>(
    cylinderMaterialSettings: &mut Option<CylinderMaterialSettings>,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *cylinderMaterialSettings = rustOptionalCylinderMaterialSettings(valueWithPath.value)?;
        return Ok(());
    }

    let Some(cylinderMaterialSettings) = cylinderMaterialSettings.as_mut() else {
        return Err(anyhow::anyhow!("update_cylinder.material: set material before updating nested fields"));
    };
    rustUpdateCylinderMaterialSettings(cylinderMaterialSettings, valueWithPath)
}

// ---------------- BrickMaterialSettings ----------------

fn jsBrickMaterialSettings<'s>(brickMaterialSettings: &BrickMaterialSettings, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let object = jsEmptyObject(scope);
    jsSetField(
        object,
        "materialInfoId",
        |s| Ok(jsNumber(ObjectId::from(brickMaterialSettings.materialInfoId), s)),
        scope,
    )?;
    jsSetField(object, "tileSizeU", |s| Ok(jsNumber(brickMaterialSettings.tileSizeU, s)), scope)?;
    jsSetField(object, "tileSizeV", |s| Ok(jsNumber(brickMaterialSettings.tileSizeV, s)), scope)?;
    jsSetField(object, "tileOffsetU", |s| Ok(jsNumber(brickMaterialSettings.tileOffsetU, s)), scope)?;
    jsSetField(object, "tileOffsetV", |s| Ok(jsNumber(brickMaterialSettings.tileOffsetV, s)), scope)?;
    Ok(object)
}

fn jsOptionalBrickMaterialSettings<'s>(
    brickMaterialSettings: &Option<BrickMaterialSettings>,
    scope: &mut PinScope<'s, '_>,
) -> anyhow::Result<Local<'s, v8::Value>> {
    match brickMaterialSettings {
        Some(brickMaterialSettings) => Ok(jsBrickMaterialSettings(brickMaterialSettings, scope)?.into()),
        None => Ok(jsNull(scope).into()),
    }
}

fn rustBrickMaterialSettings(jsvalue: &Value) -> anyhow::Result<BrickMaterialSettings> {
    let object = asJSON(jsvalue, "material")?;
    Ok(BrickMaterialSettings {
        materialInfoId: ObjectIdPointer(rustObjectId(readJSON(object, "materialInfoId", "material")?)?),
        tileSizeU: rustf32(readJSON(object, "tileSizeU", "material")?)?,
        tileSizeV: rustf32(readJSON(object, "tileSizeV", "material")?)?,
        tileOffsetU: rustf32(readJSON(object, "tileOffsetU", "material")?)?,
        tileOffsetV: rustf32(readJSON(object, "tileOffsetV", "material")?)?,
    })
}

fn rustOptionalBrickMaterialSettings(jsvalue: &Value) -> anyhow::Result<Option<BrickMaterialSettings>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(rustBrickMaterialSettings(jsvalue)?)),
    }
}

fn rustSetBrickMaterialSettings(brickMaterialSettings: &mut BrickMaterialSettings, jsvalue: &Value) -> anyhow::Result<()> {
    *brickMaterialSettings = rustBrickMaterialSettings(jsvalue)?;
    Ok(())
}

fn rustUpdateBrickMaterialSettingsField<'v, 'p>(
    brickMaterialSettings: &mut BrickMaterialSettings,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    match field {
        "materialInfoId" => brickMaterialSettings.materialInfoId = ObjectIdPointer(rustObjectId(valueWithPath.expectFinalNextValue()?)?),
        "tileSizeU" => brickMaterialSettings.tileSizeU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileSizeV" => brickMaterialSettings.tileSizeV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetU" => brickMaterialSettings.tileOffsetU = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "tileOffsetV" => brickMaterialSettings.tileOffsetV = rustf32(valueWithPath.expectFinalNextValue()?)?,
        _ => {
            return Err(anyhow::anyhow!(
                "update_brick.material: materialInfoId|tileSizeU|tileSizeV|tileOffsetU|tileOffsetV"
            ));
        }
    }
    Ok(())
}

fn rustUpdateBrickMaterialSettings<'v, 'p>(brickMaterialSettings: &mut BrickMaterialSettings, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetBrickMaterialSettings(brickMaterialSettings, valueWithPath.value)?;
        return Ok(());
    }

    let field = valueWithPath.nextFieldName("update_brick.material")?;
    rustUpdateBrickMaterialSettingsField(brickMaterialSettings, field.as_str(), valueWithPath)
}

fn rustUpdateOptionalBrickMaterialSettings<'v, 'p>(
    brickMaterialSettings: &mut Option<BrickMaterialSettings>,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *brickMaterialSettings = rustOptionalBrickMaterialSettings(valueWithPath.value)?;
        return Ok(());
    }

    let Some(brickMaterialSettings) = brickMaterialSettings.as_mut() else {
        return Err(anyhow::anyhow!("update_brick.material: set material before updating nested fields"));
    };
    rustUpdateBrickMaterialSettings(brickMaterialSettings, valueWithPath)
}

// ---------------- MeshMaterialSettings ----------------

fn jsMeshMaterialSettings<'s>(meshMaterialSettings: &MeshMaterialSettings, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let object = jsEmptyObject(scope);
    jsSetField(
        object,
        "materialInfoId",
        |s| Ok(jsNumber(ObjectId::from(meshMaterialSettings.materialInfoId), s)),
        scope,
    )?;
    Ok(object)
}

fn jsOptionalMeshMaterialSettings<'s>(
    meshMaterialSettings: &Option<MeshMaterialSettings>,
    scope: &mut PinScope<'s, '_>,
) -> anyhow::Result<Local<'s, v8::Value>> {
    match meshMaterialSettings {
        Some(meshMaterialSettings) => Ok(jsMeshMaterialSettings(meshMaterialSettings, scope)?.into()),
        None => Ok(jsNull(scope).into()),
    }
}

fn rustMeshMaterialSettings(jsvalue: &Value) -> anyhow::Result<MeshMaterialSettings> {
    let object = asJSON(jsvalue, "material")?;
    Ok(MeshMaterialSettings {
        materialInfoId: ObjectIdPointer(rustObjectId(readJSON(object, "materialInfoId", "material")?)?),
    })
}

fn rustOptionalMeshMaterialSettings(jsvalue: &Value) -> anyhow::Result<Option<MeshMaterialSettings>> {
    match jsvalue {
        Value::Null => Ok(None),
        _ => Ok(Some(rustMeshMaterialSettings(jsvalue)?)),
    }
}

fn rustSetMeshMaterialSettings(meshMaterialSettings: &mut MeshMaterialSettings, jsvalue: &Value) -> anyhow::Result<()> {
    *meshMaterialSettings = rustMeshMaterialSettings(jsvalue)?;
    Ok(())
}

fn rustUpdateMeshMaterialSettingsField<'v, 'p>(
    meshMaterialSettings: &mut MeshMaterialSettings,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    match field {
        "materialInfoId" => meshMaterialSettings.materialInfoId = ObjectIdPointer(rustObjectId(valueWithPath.expectFinalNextValue()?)?),
        _ => return Err(anyhow::anyhow!("update_mesh.material: materialInfoId")),
    }
    Ok(())
}

fn rustUpdateMeshMaterialSettings<'v, 'p>(meshMaterialSettings: &mut MeshMaterialSettings, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetMeshMaterialSettings(meshMaterialSettings, valueWithPath.value)?;
        return Ok(());
    }

    let field = valueWithPath.nextFieldName("update_mesh.material")?;
    rustUpdateMeshMaterialSettingsField(meshMaterialSettings, field.as_str(), valueWithPath)
}

fn rustUpdateOptionalMeshMaterialSettings<'v, 'p>(
    meshMaterialSettings: &mut Option<MeshMaterialSettings>,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        *meshMaterialSettings = rustOptionalMeshMaterialSettings(valueWithPath.value)?;
        return Ok(());
    }

    let Some(meshMaterialSettings) = meshMaterialSettings.as_mut() else {
        return Err(anyhow::anyhow!("update_mesh.material: set material before updating nested fields"));
    };
    rustUpdateMeshMaterialSettings(meshMaterialSettings, valueWithPath)
}

// ---------------- MaterialInfo ----------------

fn jsSpreadMaterialInfo<'s>(obj: Local<v8::Object>, material: &MaterialInfo, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    jsSetField(obj, "albedoTextureDataId", |s| Ok(jsOptionalDataId(material.albedoTextureDataId, s)), scope)?;
    jsSetField(obj, "metallicTextureDataId", |s| Ok(jsOptionalDataId(material.metallicTextureDataId, s)), scope)?;
    jsSetField(
        obj,
        "roughnessTextureDataId",
        |s| Ok(jsOptionalDataId(material.roughnessTextureDataId, s)),
        scope,
    )?;
    jsSetField(obj, "normalTextureDataId", |s| Ok(jsOptionalDataId(material.normalTextureDataId, s)), scope)?;
    jsSetField(obj, "alphaMode", |s| jsAlphaMode(material.alphaMode, s), scope)?;
    jsSetField(obj, "isDoubleSided", |s| Ok(jsBoolean(material.isDoubleSided, s)), scope)?;
    Ok(())
}

fn rustMaterialInfo(jsvalue: &Value) -> anyhow::Result<MaterialInfo> {
    let object = asJSON(jsvalue, "materialInfo")?;
    Ok(MaterialInfo {
        albedoTextureDataId: rustOptionDataId(readJSON(object, "albedoTextureDataId", "materialInfo")?)?,
        metallicTextureDataId: rustOptionDataId(readJSON(object, "metallicTextureDataId", "materialInfo")?)?,
        roughnessTextureDataId: rustOptionDataId(readJSON(object, "roughnessTextureDataId", "materialInfo")?)?,
        normalTextureDataId: rustOptionDataId(readJSON(object, "normalTextureDataId", "materialInfo")?)?,
        alphaMode: rustAlphaMode(readJSON(object, "alphaMode", "materialInfo")?)?,
        isDoubleSided: rustBool(readJSON(object, "isDoubleSided", "materialInfo")?)?,
    })
}

fn rustUpdateMaterialInfoField<'v, 'p>(material_info: &mut MaterialInfo, field: &str, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    match field {
        "albedoTextureDataId" => {
            material_info.albedoTextureDataId = rustOptionDataId(valueWithPath.expectFinalNextValue()?)?;
        }
        "metallicTextureDataId" => {
            material_info.metallicTextureDataId = rustOptionDataId(valueWithPath.expectFinalNextValue()?)?;
        }
        "roughnessTextureDataId" => {
            material_info.roughnessTextureDataId = rustOptionDataId(valueWithPath.expectFinalNextValue()?)?;
        }
        "normalTextureDataId" => {
            material_info.normalTextureDataId = rustOptionDataId(valueWithPath.expectFinalNextValue()?)?;
        }
        "isDoubleSided" => {
            material_info.isDoubleSided = rustBool(valueWithPath.expectFinalNextValue()?)?;
        }
        "alphaMode" => {
            rustUpdateAlphaMode(&mut material_info.alphaMode, valueWithPath.nextValue()?)?;
        }
        _ => {
            return Err(anyhow::anyhow!(
                "update_material: albedoTextureDataId|metallicTextureDataId|roughnessTextureDataId|normalTextureDataId|alphaMode|isDoubleSided"
            ));
        }
    }
    Ok(())
}

// ---------------- WorldLightingInfo ----------------

fn jsSpreadWorldLightingInfo<'s>(obj: Local<v8::Object>, world_lighting: &WorldLightingInfo, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    jsSetField(obj, "sunOrientation", |s| Ok(jsOrientation3D(world_lighting.sunOrientation, s)?), scope)?;
    jsSetField(obj, "sunColor", |s| Ok(jsColor3(&world_lighting.sunColor, s)?), scope)?;
    jsSetField(obj, "sunIntensity", |s| Ok(jsNumber(world_lighting.sunIntensity, s)), scope)?;
    jsSetField(obj, "sunShadowSoftness", |s| Ok(jsNumber(world_lighting.sunShadowSoftness, s)), scope)?;
    jsSetField(obj, "ambientTopColor", |s| Ok(jsColor3(&world_lighting.ambientTopColor, s)?), scope)?;
    jsSetField(obj, "ambientBottomColor", |s| Ok(jsColor3(&world_lighting.ambientBottomColor, s)?), scope)?;
    jsSetField(obj, "ambientIntensity", |s| Ok(jsNumber(world_lighting.ambientIntensity, s)), scope)?;
    jsSetField(obj, "ambientOcclusionIntensity", |s| Ok(jsNumber(world_lighting.ambientOcclusionIntensity, s)), scope)?;
    jsSetField(obj, "ambientOcclusionRadius", |s| Ok(jsNumber(world_lighting.ambientOcclusionRadius, s)), scope)?;
    jsSetField(
        obj,
        "ambientOcclusionSunlightStrength",
        |s| Ok(jsNumber(world_lighting.ambientOcclusionSunlightStrength, s)),
        scope,
    )?;
    jsSetField(obj, "skyTopColor", |s| Ok(jsColor3(&world_lighting.skyTopColor, s)?), scope)?;
    jsSetField(obj, "skyHorizonColor", |s| Ok(jsColor3(&world_lighting.skyHorizonColor, s)?), scope)?;
    jsSetField(obj, "skyBottomColor", |s| Ok(jsColor3(&world_lighting.skyBottomColor, s)?), scope)?;
    jsSetField(
        obj,
        "skyboxTextureDataId",
        |s| Ok(jsOptionalDataId(world_lighting.skyboxTextureDataId, s)),
        scope,
    )?;
    jsSetField(obj, "skyboxTextureOpacity", |s| Ok(jsNumber(world_lighting.skyboxTextureOpacity, s)), scope)?;
    jsSetField(obj, "skyReflectionStrength", |s| Ok(jsNumber(world_lighting.skyReflectionStrength, s)), scope)?;
    jsSetField(obj, "vibrance", |s| Ok(jsNumber(world_lighting.vibrance, s)), scope)?;
    Ok(())
}

fn rustWorldLightingInfo(jsvalue: &Value) -> anyhow::Result<WorldLightingInfo> {
    let object = asJSON(jsvalue, "worldLightingInfo")?;
    Ok(WorldLightingInfo {
        sunOrientation: rustOrientation3D(readJSON(object, "sunOrientation", "worldLightingInfo")?)?,
        sunColor: rustColor3(readJSON(object, "sunColor", "worldLightingInfo")?)?,
        sunIntensity: rustf32(readJSON(object, "sunIntensity", "worldLightingInfo")?)?,
        sunShadowSoftness: rustf32(readJSON(object, "sunShadowSoftness", "worldLightingInfo")?)?,
        ambientTopColor: rustColor3(readJSON(object, "ambientTopColor", "worldLightingInfo")?)?,
        ambientBottomColor: rustColor3(readJSON(object, "ambientBottomColor", "worldLightingInfo")?)?,
        ambientIntensity: rustf32(readJSON(object, "ambientIntensity", "worldLightingInfo")?)?,
        ambientOcclusionIntensity: rustf32(readJSON(object, "ambientOcclusionIntensity", "worldLightingInfo")?)?,
        ambientOcclusionRadius: rustf32(readJSON(object, "ambientOcclusionRadius", "worldLightingInfo")?)?,
        ambientOcclusionSunlightStrength: rustf32(readJSON(object, "ambientOcclusionSunlightStrength", "worldLightingInfo")?)?,
        skyTopColor: rustColor3(readJSON(object, "skyTopColor", "worldLightingInfo")?)?,
        skyHorizonColor: rustColor3(readJSON(object, "skyHorizonColor", "worldLightingInfo")?)?,
        skyBottomColor: rustColor3(readJSON(object, "skyBottomColor", "worldLightingInfo")?)?,
        skyboxTextureDataId: rustOptionDataId(readJSON(object, "skyboxTextureDataId", "worldLightingInfo")?)?,
        skyboxTextureOpacity: rustf32(readJSON(object, "skyboxTextureOpacity", "worldLightingInfo")?)?,
        skyReflectionStrength: rustf32(readJSON(object, "skyReflectionStrength", "worldLightingInfo")?)?,
        vibrance: rustf32(readJSON(object, "vibrance", "worldLightingInfo")?)?,
    })
}

fn rustUpdateWorldLightingInfoField<'v, 'p>(
    world_lighting_info: &mut WorldLightingInfo,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<()> {
    match field {
        "sunOrientation" => rustUpdateOrientation3D(&mut world_lighting_info.sunOrientation, valueWithPath.nextValue()?),
        "sunColor" => rustUpdateColor3(&mut world_lighting_info.sunColor, valueWithPath.nextValue()?),
        "sunIntensity" => {
            world_lighting_info.sunIntensity = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "sunShadowSoftness" => {
            world_lighting_info.sunShadowSoftness = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "ambientTopColor" => rustUpdateColor3(&mut world_lighting_info.ambientTopColor, valueWithPath.nextValue()?),
        "ambientBottomColor" => rustUpdateColor3(&mut world_lighting_info.ambientBottomColor, valueWithPath.nextValue()?),
        "ambientIntensity" => {
            world_lighting_info.ambientIntensity = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "ambientOcclusionIntensity" => {
            world_lighting_info.ambientOcclusionIntensity = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "ambientOcclusionRadius" => {
            world_lighting_info.ambientOcclusionRadius = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "ambientOcclusionSunlightStrength" => {
            world_lighting_info.ambientOcclusionSunlightStrength = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "skyTopColor" => rustUpdateColor3(&mut world_lighting_info.skyTopColor, valueWithPath.nextValue()?),
        "skyHorizonColor" => rustUpdateColor3(&mut world_lighting_info.skyHorizonColor, valueWithPath.nextValue()?),
        "skyBottomColor" => rustUpdateColor3(&mut world_lighting_info.skyBottomColor, valueWithPath.nextValue()?),
        "skyboxTextureDataId" => {
            world_lighting_info.skyboxTextureDataId = rustOptionDataId(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "skyboxTextureOpacity" => {
            world_lighting_info.skyboxTextureOpacity = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "skyReflectionStrength" => {
            world_lighting_info.skyReflectionStrength = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        "vibrance" => {
            world_lighting_info.vibrance = rustf32(valueWithPath.expectFinalNextValue()?)?;
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "update_world_lighting: sunOrientation|sunColor|sunIntensity|sunShadowSoftness|ambientTopColor|ambientBottomColor|ambientIntensity|ambientOcclusionIntensity|ambientOcclusionRadius|ambientOcclusionSunlightStrength|skyTopColor|skyHorizonColor|skyBottomColor|skyboxTextureDataId|skyboxTextureOpacity|skyReflectionStrength|vibrance"
        )),
    }
}

// ---------------- Rotation ----------------

fn jsRotation<'s>(q: Quat, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    jsVec3(quat_to_euler_degrees(q), scope)
}

fn rustRotation(jsvalue: &Value) -> anyhow::Result<Quat> {
    Ok(euler_degrees_to_quat(rustVec3(jsvalue)?))
}

// A write to one axis keeps the other two as they read, so it lands on `rotation_degrees`, the degrees the rotation
// reads as, instead of on a fresh decomposition of the quaternion.
fn rustUpdateRotation<'v, 'p>(quat: &mut Quat, rotation_degrees: &mut Vec3, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    rustUpdateVec3(rotation_degrees, valueWithPath)?;
    *quat = euler_degrees_to_quat(*rotation_degrees);
    Ok(())
}

// ---------------- Location3D ----------------

fn jsLocation3D<'s>(location3D: &Location3D, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    jsLocation3DWithRotationDegrees(location3D.position, quat_to_euler_degrees(location3D.quaternion), scope)
}

fn jsLocation3DWithRotationDegrees<'s>(position: Vec3, rotation_degrees: Vec3, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "position", |s| Ok(jsVec3(position, s)?), scope)?;
    jsSetField(obj, "rotation", |s| Ok(jsVec3(rotation_degrees, s)?), scope)?;
    Ok(obj)
}

fn rustLocation3D(jsvalue: &Value) -> anyhow::Result<Location3D> {
    let object = asJSON(jsvalue, "location3d")?;
    Ok(Location3D {
        position: rustVec3(readJSON(object, "position", "location3d")?)?,
        quaternion: rustRotation(readJSON(object, "rotation", "location3d")?)?,
    })
}

fn rustUpdateLocation3DField<'v, 'p>(
    location: &mut Location3D,
    rotation_degrees: &mut Vec3,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<PhysicsInfo3DFlags> {
    match field {
        "position" => {
            rustUpdateVec3(&mut location.position, valueWithPath)?;
            Ok(PhysicsInfo3DFlags::new(true, false, false, false))
        }
        "rotation" => {
            rustUpdateRotation(&mut location.quaternion, rotation_degrees, valueWithPath)?;
            Ok(PhysicsInfo3DFlags::new(false, true, false, false))
        }
        _ => return Err(anyhow::anyhow!("update_location3d: position|rotation")),
    }
}

fn rustSetLocation3D(location: &mut Location3D, rotation_degrees: &mut Vec3, jsvalue: &Value) -> anyhow::Result<PhysicsInfo3DFlags> {
    let object = asJSON(jsvalue, "location3d")?;
    let position = rustVec3(readJSON(object, "position", "location3d")?)?;
    *rotation_degrees = rustVec3(readJSON(object, "rotation", "location3d")?)?;
    *location = Location3D {
        position,
        quaternion: euler_degrees_to_quat(*rotation_degrees),
    };
    Ok(PhysicsInfo3DFlags::new(true, true, false, false))
}

fn rustUpdateLocation3D<'v, 'p>(location: &mut Location3D, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<PhysicsInfo3DFlags> {
    let mut rotation_degrees = quat_to_euler_degrees(location.quaternion);
    rustUpdateLocation3DWithRotationDegrees(location, &mut rotation_degrees, valueWithPath)
}

// Like rustUpdateLocation3D, for a location whose rotation reads as `rotation_degrees`, which it updates too.
fn rustUpdateLocation3DWithRotationDegrees<'v, 'p>(
    location: &mut Location3D,
    rotation_degrees: &mut Vec3,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<PhysicsInfo3DFlags> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetLocation3D(location, rotation_degrees, valueWithPath.value)
    } else {
        let field = valueWithPath.nextFieldName("update_location3d")?;
        let next = valueWithPath.nextValue()?;
        rustUpdateLocation3DField(location, rotation_degrees, field.as_str(), next)
    }
}

// ---------------- SkAnimation ----------------

fn jsSpreadSkAnimationInfo<'s>(obj: Local<v8::Object>, sk_animation: &SkAnimationInfo, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    jsSetField(obj, "duration", |s| Ok(jsNumber(sk_animation.duration, s)), scope)?;
    jsSetField(obj, "interpolationStyle", |s| jsInterpolationStyle(sk_animation.interpolationStyle, s), scope)?;
    Ok(())
}

fn rustSkAnimationInfo(jsvalue: &Value) -> anyhow::Result<SkAnimationInfo> {
    let object = asJSON(jsvalue, "skAnimationInfo")?;
    Ok(SkAnimationInfo {
        duration: rustf32(readJSON(object, "duration", "skAnimationInfo")?)?,
        interpolationStyle: rustInterpolationStyle(readJSON(object, "interpolationStyle", "skAnimationInfo")?, "skAnimationInfo.interpolationStyle")?,
    })
}

// ---------------- PropertyAnimation ----------------

fn rustPropertyAnimationInfo(jsvalue: &Value) -> anyhow::Result<PropertyAnimationInfo> {
    let object = asJSON(jsvalue, "propertyAnimationInfo")?;
    Ok(PropertyAnimationInfo {
        duration: rustf32(readJSON(object, "duration", "propertyAnimationInfo")?)?,
        interpolationStyle: rustInterpolationStyle(
            readJSON(object, "interpolationStyle", "propertyAnimationInfo")?,
            "propertyAnimationInfo.interpolationStyle",
        )?,
    })
}

// ---------------- Movement3D ----------------

fn jsMovement3D<'s>(movement3D: &Movement3D, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    jsSetField(obj, "velocity", |s| Ok(jsVec3(movement3D.velocity, s)?), scope)?;
    jsSetField(obj, "angularVelocity", |s| Ok(jsVec3DegreesFromRadians(movement3D.angularVelocity, s)?), scope)?;
    Ok(obj)
}

fn rustMovement3D(jsvalue: &Value) -> anyhow::Result<Movement3D> {
    let object = asJSON(jsvalue, "movement3d")?;
    Ok(Movement3D {
        velocity: rustVec3(readJSON(object, "velocity", "movement3d")?)?,
        angularVelocity: rustVec3RadiansFromDegrees(readJSON(object, "angularVelocity", "movement3d")?)?,
    })
}

fn rustUpdateMovement3DField<'v, 'p>(movement: &mut Movement3D, field: &str, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<PhysicsInfo3DFlags> {
    match field {
        "velocity" => {
            rustUpdateVec3(&mut movement.velocity, valueWithPath)?;
            Ok(PhysicsInfo3DFlags::new(false, false, true, false))
        }
        "angularVelocity" => {
            rustUpdateVec3RadiansFromDegrees(&mut movement.angularVelocity, valueWithPath)?;
            Ok(PhysicsInfo3DFlags::new(false, false, false, true))
        }
        _ => return Err(anyhow::anyhow!("update_movement3d: velocity|angularVelocity")),
    }
}

fn rustSetMovement3D(movement: &mut Movement3D, jsvalue: &Value) -> anyhow::Result<PhysicsInfo3DFlags> {
    *movement = rustMovement3D(jsvalue)?;
    Ok(PhysicsInfo3DFlags::new(false, false, true, true))
}

fn rustUpdateMovement3D<'v, 'p>(movement: &mut Movement3D, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<PhysicsInfo3DFlags> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetMovement3D(movement, valueWithPath.value)
    } else {
        let field = valueWithPath.nextFieldName("update_movement3d")?;
        let next = valueWithPath.nextValue()?;
        rustUpdateMovement3DField(movement, field.as_str(), next)
    }
}

// ---------------- Orientation3D ----------------

fn jsOrientation3D<'s>(orientation3D: Orientation3D, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let obj = jsEmptyObject(scope);
    // JS uses positive-up phi, while Rust's internal Orientation3D math uses positive-down phi.
    jsSetField(obj, "theta", |s| Ok(jsDegrees(orientation3D.theta, s)), scope)?;
    jsSetField(obj, "phi", |s| Ok(jsDegrees(-orientation3D.phi, s)), scope)?;
    Ok(obj)
}

fn rustOrientation3D(jsvalue: &Value) -> anyhow::Result<Orientation3D> {
    let object = asJSON(jsvalue, "orientation3d")?;
    Ok(Orientation3D {
        theta: rustRadiansFromDegrees(readJSON(object, "theta", "orientation3d")?)?,
        phi: -rustRadiansFromDegrees(readJSON(object, "phi", "orientation3d")?)?,
    })
}

fn rustUpdateOrientation3DField(orientation: &mut Orientation3D, field: &str, jsvalue: &Value) -> anyhow::Result<()> {
    let val = rustRadiansFromDegrees(jsvalue)?;
    match field {
        "theta" => orientation.theta = val,
        "phi" => orientation.phi = -val,
        _ => return Err(anyhow::anyhow!("update_orientation3d: theta|phi")),
    }
    Ok(())
}

fn rustSetOrientation3D(orientation: &mut Orientation3D, jsvalue: &Value) -> anyhow::Result<()> {
    *orientation = rustOrientation3D(jsvalue)?;
    Ok(())
}

fn rustUpdateOrientation3D<'v, 'p>(orientation: &mut Orientation3D, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    if valueWithPath.fullRemainingPath.is_empty() {
        rustSetOrientation3D(orientation, valueWithPath.value)
    } else {
        let field = valueWithPath.nextFieldName("update_orientation3d")?;
        let leaf_value = valueWithPath.expectFinalNextValue()?;
        rustUpdateOrientation3DField(orientation, field.as_str(), leaf_value)
    }
}

// ---------------- PhysicsToggles3D ----------------
struct PhysicsToggles3DFlags {
    isWalkthrough: bool,
    isPushable: bool,
    density: bool,
}
impl PhysicsToggles3DFlags {
    fn new(isWalkthrough: bool, isPushable: bool, density: bool) -> Self {
        Self {
            isWalkthrough,
            isPushable,
            density,
        }
    }
}

fn jsSpreadPhysicsToggles3D<'s>(obj: Local<v8::Object>, world: &World, objectId: ObjectId, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    match world.getObjectPhysicsToggles3D(objectId) {
        Some(physicsToggles3D) => {
            jsSetField(obj, "isWalkthrough", |s| Ok(jsBoolean(physicsToggles3D.isWalkthrough, s)), scope)?;
            jsSetField(obj, "isPushable", |s| Ok(jsBoolean(physicsToggles3D.isPushable, s)), scope)?;
            jsSetField(obj, "density", |s| Ok(jsNumber(physicsToggles3D.density, s)), scope)?;
            Ok(())
        }
        None => Err(anyhow::anyhow!("jsSpreadPhysicsInfo3DFromWorld: physics object not found")),
    }
}

fn rustPhysicsToggles3D(jsvalue: &Value) -> anyhow::Result<PhysicsToggles3D> {
    let object = asJSON(jsvalue, "set_physicsToggles3D")?;
    Ok(PhysicsToggles3D {
        isWalkthrough: rustBool(readJSON(object, "isWalkthrough", "set_physicsToggles3D")?)?,
        isPushable: rustBool(readJSON(object, "isPushable", "set_physicsToggles3D")?)?,
        density: object.get("density").map(rustf32).transpose()?.unwrap_or(1.0),
    })
}

fn rustUpdatePhysicsToggles3DField(physicsToggles3D: &mut PhysicsToggles3D, field: &str, jsvalue: &Value) -> anyhow::Result<PhysicsToggles3DFlags> {
    match field {
        "isWalkthrough" => {
            physicsToggles3D.isWalkthrough = rustBool(jsvalue)?;
            Ok(PhysicsToggles3DFlags::new(true, false, false))
        }
        "isPushable" => {
            physicsToggles3D.isPushable = rustBool(jsvalue)?;
            Ok(PhysicsToggles3DFlags::new(false, true, false))
        }
        "density" => {
            physicsToggles3D.density = rustf32(jsvalue)?;
            Ok(PhysicsToggles3DFlags::new(false, false, true))
        }
        _ => return Err(anyhow::anyhow!("update_physicsToggles3D: isWalkthrough|isPushable|density")),
    }
}

fn rustUpdatePhysicsToggles3D<'v, 'p>(world: &mut World, objectId: ObjectId, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    // create a physics toggles 3d rust object
    let physicsToggles3D = world.getObjectPhysicsToggles3D(objectId);
    let Some(mut physicsToggles3D) = physicsToggles3D else { return Ok(()) };

    let flags;
    if valueWithPath.fullRemainingPath.is_empty() {
        // this case should never happen
        return Err(anyhow::anyhow!("update_physicsToggles3D: fullRemainingPath is empty"));
    } else {
        let field = valueWithPath.nextFieldName("update_physicsInfo3D")?;
        flags = rustUpdatePhysicsToggles3DField(&mut physicsToggles3D, field, valueWithPath.expectFinalNextValue()?)?;
    }

    let setPhysicsToggles3D = SetPhysicsToggles3D {
        isWalkthrough: someIffTrue(flags.isWalkthrough, || physicsToggles3D.isWalkthrough),
        isPushable: someIffTrue(flags.isPushable, || physicsToggles3D.isPushable),
        density: someIffTrue(flags.density, || physicsToggles3D.density),
    };
    world.setPhysicsToggles3D(objectId, setPhysicsToggles3D, false);
    Ok(())
}

// ---------------- VisualToggles3D ----------------

fn jsSpreadVisualSettings<'s>(obj: Local<v8::Object>, visualSettings: &VisualSettings, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    jsSetField(obj, "opacity", |s| Ok(jsNumber(visualSettings.opacity, s)), scope)?;
    jsSetField(obj, "color", |s| Ok(jsColor3(&visualSettings.color, s)?), scope)?;
    jsSetField(obj, "glow", |s| Ok(jsNumber(visualSettings.glow, s)), scope)?;
    jsSetField(obj, "isAlwaysOnTop", |s| Ok(jsBoolean(visualSettings.isAlwaysOnTop, s)), scope)?;
    Ok(())
}

fn jsOptionalColor3<'s>(color3: &Option<Color3>, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Value>> {
    match color3 {
        Some(color3) => Ok(jsColor3(color3, scope)?.into()),
        None => Ok(v8::null(scope).into()),
    }
}

fn rustOptionColor3(jsvalue: &Value) -> anyhow::Result<Option<Color3>> {
    if jsvalue.is_null() {
        return Ok(None);
    }
    Ok(Some(rustColor3(jsvalue)?))
}

fn rustUpdateOptionalColor3<'v, 'p>(color3: &mut Option<Color3>, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    // A label with no background has no channel to set, so require the whole color; null clears it.
    match color3 {
        Some(existing_color3) if !valueWithPath.fullRemainingPath.is_empty() => rustUpdateColor3(existing_color3, valueWithPath),
        _ => {
            *color3 = rustOptionColor3(valueWithPath.value)?;
            Ok(())
        }
    }
}

fn rustOptionf32(jsvalue: &Value) -> anyhow::Result<Option<f32>> {
    if jsvalue.is_null() {
        return Ok(None);
    }
    Ok(Some(rustf32(jsvalue)?))
}

fn billboardModeName(billboardMode: BillboardMode) -> &'static str {
    match billboardMode {
        BillboardMode::None => "none",
        BillboardMode::Full => "full",
    }
}

fn rustBillboardMode(jsvalue: &Value) -> anyhow::Result<BillboardMode> {
    match rustString(jsvalue)?.as_str() {
        "none" => Ok(BillboardMode::None),
        "full" => Ok(BillboardMode::Full),
        other => Err(anyhow::anyhow!("billboardMode must be none|full, got \"{other}\"")),
    }
}

fn labelHorizontalAlignName(horizontalAlign: LabelHorizontalAlign) -> &'static str {
    match horizontalAlign {
        LabelHorizontalAlign::Left => "left",
        LabelHorizontalAlign::Center => "center",
        LabelHorizontalAlign::Right => "right",
    }
}

fn rustLabelHorizontalAlign(jsvalue: &Value) -> anyhow::Result<LabelHorizontalAlign> {
    match rustString(jsvalue)?.as_str() {
        "left" => Ok(LabelHorizontalAlign::Left),
        "center" => Ok(LabelHorizontalAlign::Center),
        "right" => Ok(LabelHorizontalAlign::Right),
        other => Err(anyhow::anyhow!("horizontalAlign must be left|center|right, got \"{other}\"")),
    }
}

fn rustVisualSettings(jsvalue: &Value) -> anyhow::Result<VisualSettings> {
    let object = asJSON(jsvalue, "set_visualSettings")?;
    Ok(VisualSettings {
        opacity: rustf32(readJSON(object, "opacity", "set_visualSettings")?)?,
        color: rustColor3(readJSON(object, "color", "set_visualSettings")?)?,
        glow: object.get("glow").map(rustf32).transpose()?.unwrap_or(0.0),
        isAlwaysOnTop: object.get("isAlwaysOnTop").map(rustBool).transpose()?.unwrap_or(false),
    })
}

fn rustUpdateVisualSettingsField<'v, 'p>(visualSettings: &mut VisualSettings, field: &str, valueWithPath: ValueWithPath<'v, 'p>) -> anyhow::Result<()> {
    match field {
        "opacity" => visualSettings.opacity = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "color" => rustUpdateColor3(&mut visualSettings.color, valueWithPath.nextValue()?)?,
        "glow" => visualSettings.glow = rustf32(valueWithPath.expectFinalNextValue()?)?,
        "isAlwaysOnTop" => visualSettings.isAlwaysOnTop = rustBool(valueWithPath.expectFinalNextValue()?)?,
        _ => return Err(anyhow::anyhow!("update_visualSettings: opacity|color|glow|isAlwaysOnTop")),
    }
    Ok(())
}

// ---------------- PhysicsInfo3D ----------------
struct PhysicsInfo3DFlags {
    position: bool,
    rotation: bool,
    velocity: bool,
    angular_velocity: bool,
}
impl PhysicsInfo3DFlags {
    fn new(position: bool, rotation: bool, velocity: bool, angular_velocity: bool) -> Self {
        Self {
            position,
            rotation,
            velocity,
            angular_velocity,
        }
    }
}

fn jsSpreadPhysicsInfo3D<'s>(obj: Local<v8::Object>, world: &World, objectId: ObjectId, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    match (world.getObjectPhysicsInfo3D(objectId), world.getRotationDegrees(objectId)) {
        (Some(physicsInfo3D), Some(rotation_degrees)) => {
            jsSetField(
                obj,
                "location",
                |s| Ok(jsLocation3DWithRotationDegrees(physicsInfo3D.location.position, rotation_degrees, s)?),
                scope,
            )?;
            jsSetField(obj, "movement", |s| Ok(jsMovement3D(&physicsInfo3D.movement, s)?), scope)?;
            Ok(())
        }
        _ => Err(anyhow::anyhow!("jsSpreadPhysicsInfo3DFromWorld: physics object not found")),
    }
}

fn jsSpreadPrimitiveSettings<'s>(obj: Local<v8::Object>, world: &World, objectId: ObjectId, scope: &mut PinScope<'s, '_>) -> anyhow::Result<()> {
    match world.physics.getPrimitiveSettings(objectId) {
        Some(PrimitiveSettings::Ball(ball)) => {
            jsSetField(obj, "radius", |s| Ok(jsNumber(ball.radius, s)), scope)?;
            Ok(())
        }
        Some(PrimitiveSettings::Brick(brick)) => {
            jsSetField(obj, "size", |s| Ok(jsVec3(brick.size, s)?), scope)?;
            Ok(())
        }
        Some(PrimitiveSettings::Cylinder(cylinder)) => {
            jsSetField(obj, "radius", |s| Ok(jsNumber(cylinder.radius, s)), scope)?;
            jsSetField(obj, "length", |s| Ok(jsNumber(cylinder.length, s)), scope)?;
            Ok(())
        }
        None => Err(anyhow::anyhow!("jsSpreadPrimitiveSettings: primitive info not found")),
    }
}

fn rustPhysicsInfo3D(jsvalue: &Value) -> anyhow::Result<PhysicsInfo3D> {
    let object = asJSON(jsvalue, "physicsInfo3d")?;
    Ok(PhysicsInfo3D {
        location: rustLocation3D(readJSON(object, "location", "physicsInfo3d")?)?,
        movement: rustMovement3D(readJSON(object, "movement", "physicsInfo3d")?)?,
    })
}

fn rustUpdatePhysicsInfo3DField<'v, 'p>(
    physicsInfo3D: &mut PhysicsInfo3D,
    rotation_degrees: &mut Vec3,
    field: &str,
    valueWithPath: ValueWithPath<'v, 'p>,
) -> anyhow::Result<PhysicsInfo3DFlags> {
    match field {
        "location" => rustUpdateLocation3DWithRotationDegrees(&mut physicsInfo3D.location, rotation_degrees, valueWithPath),
        "movement" => rustUpdateMovement3D(&mut physicsInfo3D.movement, valueWithPath),
        _ => Err(anyhow::anyhow!("update_physicsInfo3D: location|movement")),
    }
}
fn rustSetPhysicsInfo3D(physicsInfo3D: &mut PhysicsInfo3D, rotation_degrees: &mut Vec3, jsvalue: &Value) -> anyhow::Result<PhysicsInfo3DFlags> {
    let object = asJSON(jsvalue, "physicsInfo3d")?;
    let movement = rustMovement3D(readJSON(object, "movement", "physicsInfo3d")?)?;
    rustSetLocation3D(&mut physicsInfo3D.location, rotation_degrees, readJSON(object, "location", "physicsInfo3d")?)?;
    physicsInfo3D.movement = movement;
    Ok(PhysicsInfo3DFlags::new(true, true, true, true))
}
fn rustUpdatePhysicsInfo3D<'v, 'p>(
    world: &mut World,
    objectId: ObjectId,
    valueWithPath: ValueWithPath<'v, 'p>,
    set_or_force: SetOrForce,
) -> anyhow::Result<()> {
    // create a physics info 3d rust object
    let physicsInfo3D = world.getObjectPhysicsInfo3D(objectId);
    let Some(mut physicsInfo3D) = physicsInfo3D else { return Ok(()) };
    let Some(mut rotation_degrees) = world.getRotationDegrees(objectId) else { return Ok(()) };

    // modify the physics info 3d object, keeping track of changes as flags
    let flags;
    if valueWithPath.fullRemainingPath.is_empty() {
        flags = rustSetPhysicsInfo3D(&mut physicsInfo3D, &mut rotation_degrees, valueWithPath.value)?;
    } else {
        let field = valueWithPath.nextFieldName("update_physicsInfo3D")?;
        let next = valueWithPath.nextValue()?;
        flags = rustUpdatePhysicsInfo3DField(&mut physicsInfo3D, &mut rotation_degrees, field.as_str(), next)?;
    }

    // update the world based on the modified physics info 3d object
    let setPhysicsInfo3D = SetPhysicsInfo3D {
        position: someIffTrue(flags.position, || physicsInfo3D.location.position),
        rotation: someIffTrue(flags.rotation, || physicsInfo3D.location.quaternion),
        velocity: someIffTrue(flags.velocity, || physicsInfo3D.movement.velocity),
        angularVelocity: someIffTrue(flags.angular_velocity, || physicsInfo3D.movement.angularVelocity),
    };
    match set_or_force {
        SetOrForce::Set => world.setPhysicsInfo3D(objectId, setPhysicsInfo3D, false),
        SetOrForce::Force => world.forcePhysicsInfo3D(objectId, setPhysicsInfo3D, false),
    }
    if flags.rotation {
        world.writtenRotationDegreesOfObjectId.insert(objectId, rotation_degrees);
    }
    Ok(())
}

// ---------------- Rust -> JS ----------------

pub fn jsLiveObjectOfRustLiveObject<'s>(world: &World, object_id: ObjectId, scope: &mut PinScope<'s, '_>) -> anyhow::Result<Local<'s, v8::Object>> {
    let object = world.getObject(object_id)?;
    let js_obj = jsEmptyObject(scope);
    jsSetField(js_obj, "id", |s| Ok(jsNumber(jsObjectId(object.id), s)), scope)?;
    jsSetField(js_obj, "name", |s| Ok(jsString(&object.name, s)?), scope)?;

    // Return the parent ID; the JS proxy resolves it to a live object when read.
    let parent = match world.parentOfId.get(&object_id) {
        Some(parent_id) => jsNumber(jsObjectId(*parent_id), scope).into(),
        None => v8::null(scope).into(),
    };
    // Keep parent relationships out of object copies and JSON, as before.
    let parent_key = jsString("parent", scope)?;
    if js_obj.define_own_property(scope, parent_key.into(), parent, v8::PropertyAttribute::DONT_ENUM) != Some(true) {
        return Err(anyhow::anyhow!("Could not define JS field parent"));
    }

    match &object.info {
        /*
        - type = 'type'
        - all other properties should be pretty much the same as in rust!
        - see js definitions for what to spread and what's different from rust
        */
        ObjectInfo::Clock(clock) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("clock", s)?), scope)?;
            jsSetField(js_obj, "clockState", |s| Ok(jsClockState(clock.clockState, s)?), scope)?;
            jsSetField(js_obj, "shouldLoop", |s| Ok(jsBoolean(clock.shouldLoop, s)), scope)?;
            jsSetField(js_obj, "speed", |s| Ok(jsNumber(clock.speed, s)), scope)?;
        }
        ObjectInfo::Ball(ball) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("ball", s)?), scope)?;
            jsSetField(js_obj, "material", |s| Ok(jsOptionalBallMaterialSettings(&ball.material, s)?), scope)?;
            jsSpreadPrimitiveSettings(js_obj, world, object.id, scope)?;
            jsSpreadPhysicsToggles3D(js_obj, world, object.id, scope)?;
            jsSpreadVisualSettings(js_obj, &ball.visualSettings, scope)?;
            jsSpreadPhysicsInfo3D(js_obj, world, object.id, scope)?;
        }
        ObjectInfo::Cylinder(cylinder) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("cylinder", s)?), scope)?;
            jsSetField(js_obj, "material", |s| Ok(jsOptionalCylinderMaterialSettings(&cylinder.material, s)?), scope)?;
            jsSpreadPrimitiveSettings(js_obj, world, object.id, scope)?;
            jsSpreadPhysicsToggles3D(js_obj, world, object.id, scope)?;
            jsSpreadVisualSettings(js_obj, &cylinder.visualSettings, scope)?;
            jsSpreadPhysicsInfo3D(js_obj, world, object.id, scope)?;
        }
        ObjectInfo::SkJoint(joint) => {
            let rotation_degrees = world.getRotationDegrees(object.id).ok_or_else(|| anyhow::anyhow!("jsLiveObjectOfRustLiveObject: joint has no rotation"))?;
            jsSetField(js_obj, "type", |s| Ok(jsString("skJoint", s)?), scope)?;
            jsSetField(js_obj, "restPose", |s| Ok(jsLocation3DWithRotationDegrees(joint.restPose.position, rotation_degrees, s)?), scope)?;
        }
        ObjectInfo::Skeleton(_) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("skeleton", s)?), scope)?;
        }
        ObjectInfo::SkAnimation(sk_animation) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("skAnimation", s)?), scope)?;
            jsSpreadSkAnimationInfo(js_obj, sk_animation, scope)?;
        }
        ObjectInfo::SkAttachment(sk_attachment) => {
            let objectId = sk_attachment.objectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("skAttachment", s)?), scope)?;
            jsSetField(js_obj, "objectId", |s| Ok(jsOptionalObjectId(objectId, s)), scope)?;
            jsSetField(js_obj, "jointName", |s| Ok(jsString(&sk_attachment.jointName, s)?), scope)?;
            jsSetField(js_obj, "offsetFromJoint", |s| Ok(jsLocation3D(&sk_attachment.offsetFromJoint, s)?), scope)?;
        }
        ObjectInfo::Pivot(pivot) => {
            let physicalObjectId = pivot.physicalObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("pivot", s)?), scope)?;
            jsSetField(js_obj, "physicalObjectId", |s| Ok(jsOptionalObjectId(physicalObjectId, s)), scope)?;
            jsSetField(js_obj, "offsetFromObject", |s| Ok(jsVec3(pivot.offsetFromObject, s)?), scope)?;
        }
        ObjectInfo::SkKeyframe(sk_keyframe) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("skKeyframe", s)?), scope)?;
            jsSetField(js_obj, "time", |s| Ok(jsNumber(sk_keyframe.time, s)), scope)?;
        }
        ObjectInfo::SkKeyframePose(sk_keyframe_pose) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("skKeyframePose", s)?), scope)?;
            jsSetField(js_obj, "jointName", |s| Ok(jsString(&sk_keyframe_pose.jointName, s)?), scope)?;
            jsSetField(js_obj, "localPose", |s| Ok(jsLocation3D(&sk_keyframe_pose.localPose, s)?), scope)?;
        }
        ObjectInfo::Brick(brick) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("brick", s)?), scope)?;
            jsSetField(js_obj, "material", |s| Ok(jsOptionalBrickMaterialSettings(&brick.material, s)?), scope)?;
            jsSpreadPrimitiveSettings(js_obj, world, object.id, scope)?;
            jsSpreadPhysicsToggles3D(js_obj, world, object.id, scope)?;
            jsSpreadVisualSettings(js_obj, &brick.visualSettings, scope)?;
            jsSpreadPhysicsInfo3D(js_obj, world, object.id, scope)?;
        }
        ObjectInfo::LocalGroup(_) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("group", s)?), scope)?;
        }
        ObjectInfo::Material(material) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("material", s)?), scope)?;
            jsSpreadMaterialInfo(js_obj, material, scope)?;
        }
        ObjectInfo::WorldLighting(world_lighting) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("worldLighting", s)?), scope)?;
            jsSpreadWorldLightingInfo(js_obj, world_lighting, scope)?;
        }
        ObjectInfo::Label(label) => {
            let physicalObjectId = label.physicalObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("label", s)?), scope)?;
            jsSetField(js_obj, "text", |s| Ok(jsString(&label.text, s)?), scope)?;
            jsSetField(js_obj, "lineHeight", |s| Ok(jsNumber(label.lineHeight, s)), scope)?;
            jsSetField(js_obj, "maxWidth", |s| Ok(jsNumberOrNull(label.maxWidth, s)), scope)?;
            jsSetField(
                js_obj,
                "horizontalAlign",
                |s| Ok(jsString(labelHorizontalAlignName(label.horizontalAlign), s)?),
                scope,
            )?;
            jsSetField(js_obj, "backgroundColor", |s| Ok(jsOptionalColor3(&label.backgroundColor, s)?), scope)?;
            jsSetField(js_obj, "backgroundOpacity", |s| Ok(jsNumber(label.backgroundOpacity, s)), scope)?;
            jsSetField(js_obj, "offsetFromObject", |s| Ok(jsVec3(label.offsetFromObject, s)?), scope)?;
            jsSetField(
                js_obj,
                "rotationFromObject",
                |s| Ok(jsRotation(label.rotationFromObject, s)?),
                scope,
            )?;
            jsSetField(js_obj, "physicalObjectId", |s| Ok(jsOptionalObjectId(physicalObjectId, s)), scope)?;
            jsSetField(js_obj, "billboardMode", |s| Ok(jsString(billboardModeName(label.billboardMode), s)?), scope)?;
            jsSetField(js_obj, "isConstantSize", |s| Ok(jsBoolean(label.isConstantSize, s)), scope)?;
            jsSpreadVisualSettings(js_obj, &label.visualSettings, scope)?;
        }
        ObjectInfo::LabelImage(image) => {
            let physicalObjectId = image.physicalObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("labelImage", s)?), scope)?;
            jsSetField(js_obj, "textureDataId", |s| Ok(jsDataId(image.textureDataId, s)), scope)?;
            jsSetField(js_obj, "width", |s| Ok(jsNumber(image.width, s)), scope)?;
            jsSetField(js_obj, "height", |s| Ok(jsNumber(image.height, s)), scope)?;
            jsSetField(js_obj, "offsetFromObject", |s| Ok(jsVec3(image.offsetFromObject, s)?), scope)?;
            jsSetField(js_obj, "rotationFromObject", |s| Ok(jsRotation(image.rotationFromObject, s)?), scope)?;
            jsSetField(js_obj, "physicalObjectId", |s| Ok(jsOptionalObjectId(physicalObjectId, s)), scope)?;
            jsSetField(js_obj, "billboardMode", |s| Ok(jsString(billboardModeName(image.billboardMode), s)?), scope)?;
            jsSetField(js_obj, "isConstantSize", |s| Ok(jsBoolean(image.isConstantSize, s)), scope)?;
            jsSpreadVisualSettings(js_obj, &image.visualSettings, scope)?;
        }
        ObjectInfo::Mesh(mesh) => {
            let followSkeletonId = mesh.followSkeletonId.as_ref().map(|id| ObjectId::from(id));
            let physicalObjectId = mesh.physicalObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("mesh", s)?), scope)?;
            jsSetField(js_obj, "scale", |s| Ok(jsVec3(mesh.scale, s)?), scope)?;
            jsSetField(js_obj, "meshDataId", |s| Ok(jsDataId(mesh.meshDataId, s)), scope)?;
            jsSetField(js_obj, "followSkeletonId", |s| Ok(jsOptionalObjectId(followSkeletonId, s)), scope)?;
            jsSetField(js_obj, "physicalObjectId", |s| Ok(jsOptionalObjectId(physicalObjectId, s)), scope)?;
            jsSetField(js_obj, "material", |s| Ok(jsOptionalMeshMaterialSettings(&mesh.material, s)?), scope)?;
            jsSpreadVisualSettings(js_obj, &mesh.visualSettings, scope)?;
        }
        ObjectInfo::Player(player) => {
            let headObjectId = player.headObjectId.as_ref().map(|id| ObjectId::from(id));
            let characterObjectId = player.characterObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("player", s)?), scope)?;
            jsSetField(js_obj, "username", |s| Ok(jsString(&player.username, s)?), scope)?;
            jsSetField(js_obj, "userid", |s| Ok(jsNumberOrNull(player.userid, s)), scope)?;
            jsSetField(js_obj, "preferredColor", |s| Ok(jsOptionalColor3(&player.preferredColor, s)?), scope)?;
            jsSetField(js_obj, "equippedItemId", |s| Ok(jsNumberOrNull(player.equippedItemId, s)), scope)?;
            jsSetField(js_obj, "headObjectId", |s| Ok(jsOptionalObjectId(headObjectId, s)), scope)?;
            jsSetField(js_obj, "characterObjectId", |s| Ok(jsOptionalObjectId(characterObjectId, s)), scope)?;
        }
        ObjectInfo::RigidGroup(rigid_group) => {
            let followSkeletonId = rigid_group.followSkeletonId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("rigidGroup", s)?), scope)?;
            jsSetField(js_obj, "followSkeletonId", |s| Ok(jsOptionalObjectId(followSkeletonId, s)), scope)?;
            jsSpreadPhysicsInfo3D(js_obj, world, object.id, scope)?;
        }
        ObjectInfo::Script(script) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("script", s)?), scope)?;
            jsSetField(js_obj, "objectLocation", |s| jsObjectLocation(script.objectLocation, s), scope)?;
        }
        ObjectInfo::SoundPlayer(sound_player) => {
            let clockId = sound_player.clockId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("soundPlayer", s)?), scope)?;
            jsSetField(js_obj, "soundDataId", |s| Ok(jsDataId(sound_player.soundDataId, s)), scope)?;
            jsSetField(js_obj, "volume", |s| Ok(jsNumber(sound_player.volume, s)), scope)?;
            jsSetField(js_obj, "clockId", |s| Ok(jsOptionalObjectId(clockId, s)), scope)?;
        }
        ObjectInfo::SkAnimator(animation) => {
            let clockId = animation.clockId.as_ref().map(|id| ObjectId::from(id));
            let skeletonId = animation.skeletonId.as_ref().map(|id| ObjectId::from(id));
            let skAnimationId = animation.skAnimationId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("skAnimator", s)?), scope)?;
            jsSetField(js_obj, "skeletonId", |s| Ok(jsOptionalObjectId(skeletonId, s)), scope)?;
            jsSetField(js_obj, "clockId", |s| Ok(jsOptionalObjectId(clockId, s)), scope)?;
            jsSetField(js_obj, "skAnimationId", |s| Ok(jsOptionalObjectId(skAnimationId, s)), scope)?;
            jsSetField(js_obj, "priority", |s| Ok(jsNumber(animation.priority, s)), scope)?;
            jsSetField(js_obj, "weight", |s| Ok(jsNumber(animation.weight, s)), scope)?;
            jsSetField(
                js_obj,
                "weightTransition",
                |s| Ok(jsOptionalWeightTransition(&animation.weightTransition, s)?),
                scope,
            )?;
        }
        ObjectInfo::PropertyAnimation(property_animation) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("propertyAnimation", s)?), scope)?;
            jsSetField(js_obj, "duration", |s| Ok(jsNumber(property_animation.duration, s)), scope)?;
            jsSetField(
                js_obj,
                "interpolationStyle",
                |s| jsInterpolationStyle(property_animation.interpolationStyle, s),
                scope,
            )?;
        }
        ObjectInfo::PropertyKeyframe(property_keyframe) => {
            jsSetField(js_obj, "type", |s| Ok(jsString("propertyKeyframe", s)?), scope)?;
            jsSetField(js_obj, "time", |s| Ok(jsNumber(property_keyframe.time, s)), scope)?;
            jsSetField(js_obj, "value", |s| Ok(jsNumber(property_keyframe.value, s)), scope)?;
        }
        ObjectInfo::PropertyAnimator(animation) => {
            let clockId = animation.clockId.as_ref().map(|id| ObjectId::from(id));
            let targetObjectId = animation.targetObjectId.as_ref().map(|id| ObjectId::from(id));
            let propertyAnimationId = animation.propertyAnimationId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("propertyAnimator", s)?), scope)?;
            jsSetField(js_obj, "targetObjectId", |s| Ok(jsOptionalObjectId(targetObjectId, s)), scope)?;
            jsSetField(js_obj, "propertyPath", |s| Ok(jsStringArray(&animation.propertyPath, s)?), scope)?;
            jsSetField(js_obj, "propertyAnimationId", |s| Ok(jsOptionalObjectId(propertyAnimationId, s)), scope)?;
            jsSetField(js_obj, "clockId", |s| Ok(jsOptionalObjectId(clockId, s)), scope)?;
            jsSetField(js_obj, "priority", |s| Ok(jsNumber(animation.priority, s)), scope)?;
            jsSetField(js_obj, "weight", |s| Ok(jsNumber(animation.weight, s)), scope)?;
            jsSetField(
                js_obj,
                "weightTransition",
                |s| Ok(jsOptionalWeightTransition(&animation.weightTransition, s)?),
                scope,
            )?;
        }
        ObjectInfo::Tween(tween) => {
            let targetObjectId = tween.targetObjectId.as_ref().map(|id| ObjectId::from(id));
            jsSetField(js_obj, "type", |s| Ok(jsString("tween", s)?), scope)?;
            jsSetField(js_obj, "targetObjectId", |s| Ok(jsOptionalObjectId(targetObjectId, s)), scope)?;
            jsSetField(js_obj, "propertyPath", |s| Ok(jsStringArray(&tween.propertyPath, s)?), scope)?;
            jsSetField(js_obj, "fromValue", |s| Ok(jsNumber(tween.fromValue, s)), scope)?;
            jsSetField(js_obj, "toValue", |s| Ok(jsNumber(tween.toValue, s)), scope)?;
            jsSetField(js_obj, "duration", |s| Ok(jsNumber(tween.duration, s)), scope)?;
            jsSetField(js_obj, "interpolationStyle", |s| jsInterpolationStyle(tween.interpolationStyle, s), scope)?;
            jsSetField(js_obj, "clockState", |s| Ok(jsClockState(tween.clockState, s)?), scope)?;
        }
    }

    Ok(js_obj)
}

// ---------------- JS -> Rust ----------------

pub fn rustObjectBeingAddedOfJSObjectInfo(jsvalue: &Value) -> anyhow::Result<ObjectBeingAdded> {
    let ctx = "rustObjectBeingAdded";
    let object = asJSON(jsvalue, ctx)?;
    let object_type = rustString(readJSON(object, "type", ctx)?)?;

    let info = match object_type.as_str() {
        "clock" => ObjectBeingAdded::Clock(ClockInfo {
            clockState: rustClockState(readJSON(object, "clockState", ctx)?, "clock.clockState")?,
            shouldLoop: rustBool(readJSON(object, "shouldLoop", ctx)?)?,
            speed: rustf32(readJSON(object, "speed", ctx)?)?,
        }),
        "ball" => ObjectBeingAdded::Ball(
            BallInfo {
                material: rustOptionalBallMaterialSettings(readJSON(object, "material", ctx)?)?,
                visualSettings: rustVisualSettings(jsvalue)?,
            },
            BallPrimitiveSettings {
                radius: rustf32(readJSON(object, "radius", ctx)?)?,
            },
            rustPhysicsInfo3D(jsvalue)?,
            rustPhysicsToggles3D(jsvalue)?,
        ),
        "cylinder" => ObjectBeingAdded::Cylinder(
            CylinderInfo {
                material: rustOptionalCylinderMaterialSettings(readJSON(object, "material", ctx)?)?,
                visualSettings: rustVisualSettings(jsvalue)?,
            },
            CylinderPrimitiveSettings {
                radius: rustf32(readJSON(object, "radius", ctx)?)?,
                length: rustf32(readJSON(object, "length", ctx)?)?,
            },
            rustPhysicsInfo3D(jsvalue)?,
            rustPhysicsToggles3D(jsvalue)?,
        ),
        "skJoint" => ObjectBeingAdded::SkJoint(SkJointInfo {
            restPose: rustLocation3D(readJSON(object, "restPose", ctx)?)?,
        }),
        "skeleton" => ObjectBeingAdded::Skeleton(SkeletonInfo {}),
        "skAnimation" => ObjectBeingAdded::SkAnimation(rustSkAnimationInfo(jsvalue)?),
        "skAttachment" => ObjectBeingAdded::SkAttachment(SkAttachmentInfo {
            objectId: rustOptionObjectId(readJSON(object, "objectId", ctx)?)?.map(ObjectIdPointer),
            jointName: rustString(readJSON(object, "jointName", ctx)?)?,
            offsetFromJoint: rustLocation3D(readJSON(object, "offsetFromJoint", ctx)?)?,
        }),
        "pivot" => ObjectBeingAdded::Pivot(PivotInfo {
            physicalObjectId: rustOptionObjectId(readJSON(object, "physicalObjectId", ctx)?)?.map(ObjectIdPointer),
            offsetFromObject: rustVec3(readJSON(object, "offsetFromObject", ctx)?)?,
        }),
        "skKeyframe" => ObjectBeingAdded::SkKeyframe(SkKeyframeInfo {
            time: rustf32(readJSON(object, "time", ctx)?)?,
        }),
        "skKeyframePose" => ObjectBeingAdded::SkKeyframePose(SkKeyframePoseInfo {
            jointName: rustString(readJSON(object, "jointName", ctx)?)?,
            localPose: rustLocation3D(readJSON(object, "localPose", ctx)?)?,
        }),
        "brick" => ObjectBeingAdded::Brick(
            BrickInfo {
                material: rustOptionalBrickMaterialSettings(readJSON(object, "material", ctx)?)?,
                visualSettings: rustVisualSettings(jsvalue)?,
            },
            BrickPrimitiveSettings {
                size: rustVec3(readJSON(object, "size", ctx)?)?,
            },
            rustPhysicsInfo3D(jsvalue)?,
            rustPhysicsToggles3D(jsvalue)?,
        ),
        "group" => ObjectBeingAdded::LocalGroup(LocalGroupInfo {}),
        "material" => ObjectBeingAdded::Material(rustMaterialInfo(jsvalue)?),
        "worldLighting" => ObjectBeingAdded::WorldLighting(rustWorldLightingInfo(jsvalue)?),
        "label" => ObjectBeingAdded::Label(LabelInfo {
            text: rustString(readJSON(object, "text", ctx)?)?,
            lineHeight: rustf32(readJSON(object, "lineHeight", ctx)?)?,
            maxWidth: rustOptionf32(readJSON(object, "maxWidth", ctx)?)?,
            horizontalAlign: rustLabelHorizontalAlign(readJSON(object, "horizontalAlign", ctx)?)?,
            backgroundColor: rustOptionColor3(readJSON(object, "backgroundColor", ctx)?)?,
            backgroundOpacity: rustf32(readJSON(object, "backgroundOpacity", ctx)?)?,
            offsetFromObject: rustVec3(readJSON(object, "offsetFromObject", ctx)?)?,
            rotationFromObject: rustRotation(readJSON(object, "rotationFromObject", ctx)?)?,
            physicalObjectId: rustOptionObjectId(readJSON(object, "physicalObjectId", ctx)?)?.map(ObjectIdPointer),
            billboardMode: rustBillboardMode(readJSON(object, "billboardMode", ctx)?)?,
            isConstantSize: rustBool(readJSON(object, "isConstantSize", ctx)?)?,
            visualSettings: rustVisualSettings(jsvalue)?,
        }),
        "labelImage" => ObjectBeingAdded::LabelImage(LabelImageInfo {
            textureDataId: rustDataId(readJSON(object, "textureDataId", ctx)?)?,
            width: rustf32(readJSON(object, "width", ctx)?)?,
            height: rustf32(readJSON(object, "height", ctx)?)?,
            offsetFromObject: rustVec3(readJSON(object, "offsetFromObject", ctx)?)?,
            rotationFromObject: rustRotation(readJSON(object, "rotationFromObject", ctx)?)?,
            physicalObjectId: rustOptionObjectId(readJSON(object, "physicalObjectId", ctx)?)?.map(ObjectIdPointer),
            billboardMode: rustBillboardMode(readJSON(object, "billboardMode", ctx)?)?,
            isConstantSize: rustBool(readJSON(object, "isConstantSize", ctx)?)?,
            visualSettings: rustVisualSettings(jsvalue)?,
        }),
        "mesh" => ObjectBeingAdded::Mesh(MeshInfo {
            scale: rustVec3(readJSON(object, "scale", ctx)?)?,
            meshDataId: rustDataId(readJSON(object, "meshDataId", ctx)?)?,
            followSkeletonId: rustOptionObjectId(readJSON(object, "followSkeletonId", ctx)?)?.map(ObjectIdPointer),
            physicalObjectId: rustOptionObjectId(readJSON(object, "physicalObjectId", ctx)?)?.map(ObjectIdPointer),
            material: rustOptionalMeshMaterialSettings(readJSON(object, "material", ctx)?)?,
            visualSettings: rustVisualSettings(jsvalue)?,
        }),
        "player" => ObjectBeingAdded::Player(PlayerInfo {
            username: rustString(readJSON(object, "username", ctx)?)?,
            userid: rustOptioni64(readJSON(object, "userid", ctx)?)?,
            preferredColor: rustOptionColor3(readJSON(object, "preferredColor", ctx)?)?,
            equippedItemId: rustOptioni64(readJSON(object, "equippedItemId", ctx)?)?,
            headObjectId: rustOptionObjectId(readJSON(object, "headObjectId", ctx)?)?.map(ObjectIdPointer),
            characterObjectId: rustOptionObjectId(readJSON(object, "characterObjectId", ctx)?)?.map(ObjectIdPointer),
        }),
        "rigidGroup" => ObjectBeingAdded::RigidGroup(
            RigidGroupInfo {
                followSkeletonId: rustOptionObjectId(readJSON(object, "followSkeletonId", ctx)?)?.map(ObjectIdPointer),
            },
            rustPhysicsInfo3D(jsvalue)?,
        ),
        "script" => ObjectBeingAdded::Script(ScriptInfo {
            objectLocation: rustObjectLocation(readJSON(object, "objectLocation", ctx)?, "script.objectLocation")?,
        }),
        "soundPlayer" => ObjectBeingAdded::SoundPlayer(SoundPlayerInfo {
            soundDataId: rustDataId(readJSON(object, "soundDataId", ctx)?)?,
            volume: rustf32(readJSON(object, "volume", ctx)?)?,
            clockId: rustOptionObjectId(readJSON(object, "clockId", ctx)?)?.map(ObjectIdPointer),
        }),
        "skAnimator" => ObjectBeingAdded::SkAnimator(SkAnimatorInfo {
            skeletonId: rustOptionObjectId(readJSON(object, "skeletonId", ctx)?)?.map(ObjectIdPointer),
            clockId: rustOptionObjectId(readJSON(object, "clockId", ctx)?)?.map(ObjectIdPointer),
            skAnimationId: rustOptionObjectId(readJSON(object, "skAnimationId", ctx)?)?.map(ObjectIdPointer),
            priority: rustAnimatorPriority(readJSON(object, "priority", ctx)?)?,
            weight: rustAnimationWeight(readJSON(object, "weight", ctx)?)?,
            weightTransition: rustOptionalWeightTransition(readJSON(object, "weightTransition", ctx)?, "skAnimator.weightTransition")?,
        }),
        "propertyAnimation" => ObjectBeingAdded::PropertyAnimation(rustPropertyAnimationInfo(jsvalue)?),
        "propertyKeyframe" => ObjectBeingAdded::PropertyKeyframe(PropertyKeyframeInfo {
            time: rustf32(readJSON(object, "time", ctx)?)?,
            value: rustf32(readJSON(object, "value", ctx)?)?,
        }),
        "propertyAnimator" => ObjectBeingAdded::PropertyAnimator(PropertyAnimatorInfo {
            targetObjectId: rustOptionObjectId(readJSON(object, "targetObjectId", ctx)?)?.map(ObjectIdPointer),
            propertyPath: rustStringArray(readJSON(object, "propertyPath", ctx)?, "propertyAnimator.propertyPath")?,
            propertyAnimationId: rustOptionObjectId(readJSON(object, "propertyAnimationId", ctx)?)?.map(ObjectIdPointer),
            clockId: rustOptionObjectId(readJSON(object, "clockId", ctx)?)?.map(ObjectIdPointer),
            priority: rustAnimatorPriority(readJSON(object, "priority", ctx)?)?,
            weight: rustAnimationWeight(readJSON(object, "weight", ctx)?)?,
            weightTransition: rustOptionalWeightTransition(readJSON(object, "weightTransition", ctx)?, "propertyAnimator.weightTransition")?,
        }),
        "tween" => ObjectBeingAdded::Tween(TweenInfo {
            targetObjectId: rustOptionObjectId(readJSON(object, "targetObjectId", ctx)?)?.map(ObjectIdPointer),
            propertyPath: rustStringArray(readJSON(object, "propertyPath", ctx)?, "tween.propertyPath")?,
            fromValue: rustf32(readJSON(object, "fromValue", ctx)?)?,
            toValue: rustf32(readJSON(object, "toValue", ctx)?)?,
            duration: rustf32(readJSON(object, "duration", ctx)?)?,
            interpolationStyle: rustInterpolationStyle(readJSON(object, "interpolationStyle", ctx)?, "tween.interpolationStyle")?,
            clockState: rustClockState(readJSON(object, "clockState", ctx)?, "tween.clockState")?,
        }),
        _ => return Err(anyhow::anyhow!("rustObjectBeingAdded: unknown type {}", object_type)),
    };
    // A new shape is held to the same sizes as a resize, so a saved world always opens again.
    let primitive_settings = match &info {
        ObjectBeingAdded::Ball(_, ball_primitive_settings, _, _) => Some(PrimitiveSettings::Ball(*ball_primitive_settings)),
        ObjectBeingAdded::Brick(_, brick_primitive_settings, _, _) => Some(PrimitiveSettings::Brick(*brick_primitive_settings)),
        ObjectBeingAdded::Cylinder(_, cylinder_primitive_settings, _, _) => Some(PrimitiveSettings::Cylinder(*cylinder_primitive_settings)),
        _ => None,
    };
    if let Some(primitive_settings) = primitive_settings {
        primitive_settings.validate()?;
    }

    Ok(info)
}

// ---------------- JS -> Rust (for Updates) ----------------

pub fn rustUpdateObject<'v, 'p>(
    world: &mut World,
    objectId: ObjectId,
    valueWithPath: ValueWithPath<'v, 'p>,
    set_or_force: SetOrForce,
) -> anyhow::Result<ObjectModifyFlags> {
    let nextField = valueWithPath.nextFieldName("update_object")?;

    if nextField == "name" {
        world.setObjectName(objectId, rustString(valueWithPath.expectFinalNextValue()?)?)?;
        return Ok(ObjectModifyFlags::default());
    }

    if nextField == "parent" {
        world.setParent(objectId, rustObjectId(valueWithPath.expectFinalNextValue()?)?)?;
        return Ok(ObjectModifyFlags::default());
    }

    // these flags are based on the top level only, makes it much easier
    let mut objectModifyFlags = ObjectModifyFlags::default();

    let oldTrackedFields = {
        let rust_obj = world.getObject(objectId)?;
        ObjectInfoTrackedFields::from_object_info(&rust_obj.info)
    };

    let rustObj = world.getObjectMut(objectId)?;
    match &mut rustObj.info {
        ObjectInfo::Clock(clock) => match nextField.as_str() {
            "clockState" => {
                rustUpdateClockState(&mut clock.clockState, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "shouldLoop" => {
                clock.shouldLoop = rustBool(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "speed" => {
                clock.speed = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_clock: clockState|shouldLoop|speed")),
        },
        ObjectInfo::Ball(ball) => match nextField.as_str() {
            "material" => {
                rustUpdateOptionalBallMaterialSettings(&mut ball.material, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "radius" => {
                let radius = rustf32(valueWithPath.expectFinalNextValue()?)?;
                world.setPrimitiveSettings(objectId, PrimitiveSettings::Ball(BallPrimitiveSettings { radius }), false)?;
                objectModifyFlags.modifiedPrimitiveSettings = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut ball.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "isWalkthrough" | "isPushable" | "density" => {
                rustUpdatePhysicsToggles3D(world, objectId, valueWithPath)?;
                objectModifyFlags.modifiedPhysicsToggles3D = true;
            }
            "location" | "movement" => {
                rustUpdatePhysicsInfo3D(world, objectId, valueWithPath, set_or_force)?;
                objectModifyFlags.modifiedPhysicsInfo3D = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_ball: location|movement|material|radius|opacity|color|glow|isAlwaysOnTop|isWalkthrough|isPushable|density"
                ));
            }
        },
        ObjectInfo::Cylinder(cylinder) => match nextField.as_str() {
            "material" => {
                rustUpdateOptionalCylinderMaterialSettings(&mut cylinder.material, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "radius" => {
                let Some(cylinder_primitive_settings) = world.physics.getCylinderPrimitiveSettings(objectId) else {
                    return Err(anyhow::anyhow!("update_cylinder: primitive info not found"));
                };
                let radius = rustf32(valueWithPath.expectFinalNextValue()?)?;
                world.setPrimitiveSettings(
                    objectId,
                    PrimitiveSettings::Cylinder(CylinderPrimitiveSettings {
                        radius,
                        length: cylinder_primitive_settings.length,
                    }),
                    false,
                )?;
                objectModifyFlags.modifiedPrimitiveSettings = true;
            }
            "length" => {
                let Some(cylinder_primitive_settings) = world.physics.getCylinderPrimitiveSettings(objectId) else {
                    return Err(anyhow::anyhow!("update_cylinder: primitive info not found"));
                };
                let length = rustf32(valueWithPath.expectFinalNextValue()?)?;
                world.setPrimitiveSettings(
                    objectId,
                    PrimitiveSettings::Cylinder(CylinderPrimitiveSettings {
                        radius: cylinder_primitive_settings.radius,
                        length,
                    }),
                    false,
                )?;
                objectModifyFlags.modifiedPrimitiveSettings = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut cylinder.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "isWalkthrough" | "isPushable" | "density" => {
                rustUpdatePhysicsToggles3D(world, objectId, valueWithPath)?;
                objectModifyFlags.modifiedPhysicsToggles3D = true;
            }
            "location" | "movement" => {
                rustUpdatePhysicsInfo3D(world, objectId, valueWithPath, set_or_force)?;
                objectModifyFlags.modifiedPhysicsInfo3D = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_cylinder: location|movement|material|radius|length|opacity|color|glow|isAlwaysOnTop|isWalkthrough|isPushable|density"
                ));
            }
        },
        ObjectInfo::SkJoint(_) => match nextField.as_str() {
            "restPose" => {
                // like location.rotation, a joint's restPose.rotation builds on the degrees it reads back as, and remembers the ones written to it
                let mut rotation_degrees = world.getRotationDegrees(objectId).ok_or_else(|| anyhow::anyhow!("update_sk_joint: joint has no rotation"))?;
                let ObjectInfo::SkJoint(joint) = &mut world.getObjectMut(objectId)?.info else {
                    return Err(anyhow::anyhow!("update_sk_joint: not a joint"));
                };
                let flags = rustUpdateLocation3DWithRotationDegrees(&mut joint.restPose, &mut rotation_degrees, valueWithPath.nextValue()?)?;
                if flags.rotation {
                    world.writtenRotationDegreesOfObjectId.insert(objectId, rotation_degrees);
                }
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_sk_joint: restPose")),
        },
        ObjectInfo::Skeleton(_) => return Err(anyhow::anyhow!("update_skeleton: no updatable fields")),
        ObjectInfo::SkAnimation(sk_animation) => match nextField.as_str() {
            "duration" => {
                sk_animation.duration = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "interpolationStyle" => {
                sk_animation.interpolationStyle = rustInterpolationStyle(valueWithPath.expectFinalNextValue()?, "update_sk_animation.interpolationStyle")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_sk_animation: duration|interpolationStyle")),
        },
        ObjectInfo::SkAttachment(sk_attachment) => match nextField.as_str() {
            "objectId" => {
                sk_attachment.objectId = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "jointName" => {
                sk_attachment.jointName = rustString(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "offsetFromJoint" => {
                rustUpdateLocation3D(&mut sk_attachment.offsetFromJoint, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_sk_attachment: objectId|jointName|offsetFromJoint")),
        },
        ObjectInfo::Pivot(pivot) => match nextField.as_str() {
            "physicalObjectId" => {
                pivot.physicalObjectId = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "offsetFromObject" => {
                rustUpdateVec3(&mut pivot.offsetFromObject, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_pivot: physicalObjectId|offsetFromObject")),
        },
        ObjectInfo::SkKeyframe(sk_keyframe) => match nextField.as_str() {
            "time" => {
                sk_keyframe.time = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_sk_keyframe: time")),
        },
        ObjectInfo::SkKeyframePose(sk_keyframe_pose) => match nextField.as_str() {
            "jointName" => {
                sk_keyframe_pose.jointName = rustString(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "localPose" => {
                rustUpdateLocation3D(&mut sk_keyframe_pose.localPose, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_sk_keyframe_pose: jointName|localPose")),
        },
        ObjectInfo::Brick(brick) => match nextField.as_str() {
            "material" => {
                rustUpdateOptionalBrickMaterialSettings(&mut brick.material, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "size" => {
                let Some(brick_primitive_settings) = world.physics.getBrickPrimitiveSettings(objectId) else {
                    return Err(anyhow::anyhow!("update_brick: primitive info not found"));
                };
                let mut size = brick_primitive_settings.size;
                rustUpdateVec3(&mut size, valueWithPath.nextValue()?)?;
                world.setPrimitiveSettings(objectId, PrimitiveSettings::Brick(BrickPrimitiveSettings { size }), false)?;
                objectModifyFlags.modifiedPrimitiveSettings = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut brick.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "isWalkthrough" | "isPushable" | "density" => {
                rustUpdatePhysicsToggles3D(world, objectId, valueWithPath)?;
                objectModifyFlags.modifiedPhysicsToggles3D = true;
            }
            "location" | "movement" => {
                rustUpdatePhysicsInfo3D(world, objectId, valueWithPath, set_or_force)?;
                objectModifyFlags.modifiedPhysicsInfo3D = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_brick: location|movement|material|size|opacity|color|glow|isAlwaysOnTop|isWalkthrough|isPushable|density"
                ));
            }
        },
        ObjectInfo::LocalGroup(_) => {
            return Err(anyhow::anyhow!("update_group: no editable fields"));
        }
        ObjectInfo::Material(material) => {
            rustUpdateMaterialInfoField(material, nextField.as_str(), valueWithPath)?;
            objectModifyFlags.modifiedObjectInfo = true;
        }
        ObjectInfo::WorldLighting(world_lighting) => {
            rustUpdateWorldLightingInfoField(world_lighting, nextField.as_str(), valueWithPath)?;
            objectModifyFlags.modifiedObjectInfo = true;
        }
        ObjectInfo::Mesh(mesh) => match nextField.as_str() {
            "scale" => {
                rustUpdateVec3(&mut mesh.scale, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "meshDataId" => {
                mesh.meshDataId = rustDataId(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "followSkeletonId" => {
                mesh.followSkeletonId = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "physicalObjectId" => {
                let new_physical_object_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                mesh.physicalObjectId = new_physical_object_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "material" => {
                rustUpdateOptionalMeshMaterialSettings(&mut mesh.material, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut mesh.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_mesh: scale|meshDataId|followSkeletonId|physicalObjectId|material|opacity|color|glow|isAlwaysOnTop"
                ));
            }
        },
        ObjectInfo::Label(label) => match nextField.as_str() {
            "text" => {
                label.text = LabelInfo::text_without_emoji(&rustString(valueWithPath.expectFinalNextValue()?)?);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "lineHeight" => {
                label.lineHeight = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "maxWidth" => {
                label.maxWidth = rustOptionf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "horizontalAlign" => {
                label.horizontalAlign = rustLabelHorizontalAlign(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "backgroundColor" => {
                rustUpdateOptionalColor3(&mut label.backgroundColor, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "backgroundOpacity" => {
                label.backgroundOpacity = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "offsetFromObject" => {
                rustUpdateVec3(&mut label.offsetFromObject, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "rotationFromObject" => {
                let mut rotation_degrees = quat_to_euler_degrees(label.rotationFromObject);
                rustUpdateRotation(&mut label.rotationFromObject, &mut rotation_degrees, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "physicalObjectId" => {
                let new_physical_object_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                label.physicalObjectId = new_physical_object_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "billboardMode" => {
                label.billboardMode = rustBillboardMode(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "isConstantSize" => {
                label.isConstantSize = rustBool(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut label.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_label: text|lineHeight|maxWidth|horizontalAlign|backgroundColor|backgroundOpacity|offsetFromObject|rotationFromObject|physicalObjectId|opacity|color|glow|isAlwaysOnTop|billboardMode|isConstantSize"
                ));
            }
        },
        ObjectInfo::LabelImage(image) => match nextField.as_str() {
            "textureDataId" => {
                image.textureDataId = rustDataId(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "width" => {
                image.width = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "height" => {
                image.height = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "offsetFromObject" => {
                rustUpdateVec3(&mut image.offsetFromObject, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "rotationFromObject" => {
                let mut rotation_degrees = quat_to_euler_degrees(image.rotationFromObject);
                rustUpdateRotation(&mut image.rotationFromObject, &mut rotation_degrees, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "physicalObjectId" => {
                image.physicalObjectId = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "billboardMode" => {
                image.billboardMode = rustBillboardMode(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "isConstantSize" => {
                image.isConstantSize = rustBool(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "opacity" | "color" | "glow" | "isAlwaysOnTop" => {
                rustUpdateVisualSettingsField(&mut image.visualSettings, nextField.as_str(), valueWithPath)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_label_image: textureDataId|width|height|offsetFromObject|rotationFromObject|physicalObjectId|opacity|color|glow|isAlwaysOnTop|billboardMode|isConstantSize"
                ));
            }
        },
        ObjectInfo::Player(player) => match nextField.as_str() {
            "username" => {
                player.username = rustString(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "userid" => {
                return Err(anyhow::anyhow!("update_player: userid is not editable"));
                // player.userid = rustString(valueWithPath.expectFinalNextValue()?)?;
                // objectModifyFlags.modifiedObjectInfo = true;
            }
            "preferredColor" => {
                rustUpdateOptionalColor3(&mut player.preferredColor, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "equippedItemId" => {
                player.equippedItemId = rustOptioni64(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "headObjectId" => {
                let new_head_object_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                player.headObjectId = new_head_object_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "characterObjectId" => {
                let new_character_object_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                player.characterObjectId = new_character_object_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_player: username|preferredColor|equippedItemId|headObjectId|characterObjectId"
                ));
            }
        },
        ObjectInfo::RigidGroup(rigid_group) => match nextField.as_str() {
            "location" | "movement" => {
                rustUpdatePhysicsInfo3D(world, objectId, valueWithPath, set_or_force)?;
                objectModifyFlags.modifiedPhysicsInfo3D = true;
            }
            "followSkeletonId" => {
                let new_follow_skeleton_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                rigid_group.followSkeletonId = new_follow_skeleton_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!("update_rigid_group: location|movement|followSkeletonId"));
            }
        },
        ObjectInfo::Script(script) => match nextField.as_str() {
            "objectLocation" => {
                script.objectLocation = rustObjectLocation(valueWithPath.expectFinalNextValue()?, "update_script.objectLocation")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!("update_script: objectLocation"));
            }
        },
        ObjectInfo::SkAnimator(animation) => match nextField.as_str() {
            "skeletonId" => {
                let new_skeleton_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.skeletonId = new_skeleton_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "clockId" => {
                let new_clock_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.clockId = new_clock_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "skAnimationId" => {
                let new_sk_animation_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.skAnimationId = new_sk_animation_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "priority" => {
                animation.priority = rustAnimatorPriority(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "weight" => {
                animation.weight = rustAnimationWeight(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "weightTransition" => {
                rustUpdateOptionalWeightTransition(
                    &mut animation.weightTransition,
                    valueWithPath.nextValue()?,
                    "update_sk_animator.weightTransition",
                )?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_sk_animator: skeletonId|clockId|skAnimationId|priority|weight|weightTransition"
                ));
            }
        },
        ObjectInfo::PropertyAnimation(property_animation) => match nextField.as_str() {
            "duration" => {
                property_animation.duration = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "interpolationStyle" => {
                property_animation.interpolationStyle =
                    rustInterpolationStyle(valueWithPath.expectFinalNextValue()?, "update_property_animation.interpolationStyle")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_property_animation: duration|interpolationStyle")),
        },
        ObjectInfo::PropertyKeyframe(property_keyframe) => match nextField.as_str() {
            "time" => {
                property_keyframe.time = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "value" => {
                property_keyframe.value = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_property_keyframe: time|value")),
        },
        ObjectInfo::PropertyAnimator(animation) => match nextField.as_str() {
            "targetObjectId" => {
                let new_target_object_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.targetObjectId = new_target_object_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "propertyPath" => {
                animation.propertyPath = rustStringArray(valueWithPath.expectFinalNextValue()?, "update_property_animator.propertyPath")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "propertyAnimationId" => {
                let new_property_animation_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.propertyAnimationId = new_property_animation_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "clockId" => {
                let new_clock_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                animation.clockId = new_clock_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "priority" => {
                animation.priority = rustAnimatorPriority(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "weight" => {
                animation.weight = rustAnimationWeight(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "weightTransition" => {
                rustUpdateOptionalWeightTransition(
                    &mut animation.weightTransition,
                    valueWithPath.nextValue()?,
                    "update_property_animator.weightTransition",
                )?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "update_property_animator: targetObjectId|propertyPath|propertyAnimationId|clockId|priority|weight|weightTransition"
                ));
            }
        },
        ObjectInfo::Tween(tween) => match nextField.as_str() {
            "targetObjectId" => {
                tween.targetObjectId = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "propertyPath" => {
                tween.propertyPath = rustStringArray(valueWithPath.expectFinalNextValue()?, "update_tween.propertyPath")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "fromValue" => {
                tween.fromValue = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "toValue" => {
                tween.toValue = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "duration" => {
                tween.duration = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "interpolationStyle" => {
                tween.interpolationStyle = rustInterpolationStyle(valueWithPath.expectFinalNextValue()?, "update_tween.interpolationStyle")?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "clockState" => {
                rustUpdateClockState(&mut tween.clockState, valueWithPath.nextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => return Err(anyhow::anyhow!("update_tween: targetObjectId|propertyPath|fromValue|toValue|duration|interpolationStyle|clockState")),
        },
        ObjectInfo::SoundPlayer(sound_player) => match nextField.as_str() {
            "soundDataId" => {
                sound_player.soundDataId = rustDataId(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "volume" => {
                sound_player.volume = rustf32(valueWithPath.expectFinalNextValue()?)?;
                objectModifyFlags.modifiedObjectInfo = true;
            }
            "clockId" => {
                let new_clock_id = rustOptionObjectId(valueWithPath.expectFinalNextValue()?)?;
                sound_player.clockId = new_clock_id.map(ObjectIdPointer);
                objectModifyFlags.modifiedObjectInfo = true;
            }
            _ => {
                return Err(anyhow::anyhow!("update_sound_player: soundDataId|volume|clockId"));
            }
        },
    };

    let newTrackedFields = {
        let rust_obj = world.getObject(objectId)?;
        ObjectInfoTrackedFields::from_object_info(&rust_obj.info)
    };

    world.afterObjectInfoChange(objectId, oldTrackedFields, newTrackedFields);
    Ok(objectModifyFlags)
}
