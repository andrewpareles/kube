use glam::Vec3;
use serde_json::{Value, json};
use shared::conversions::float_conversions::{short_f64_of_f32, with_short_floats};

#[test]
fn short_f64_of_f32_is_the_shortest_decimal_of_the_f32() {
    assert_eq!(short_f64_of_f32(1.1), 1.1);
    assert_eq!(short_f64_of_f32(0.3), 0.3);
    assert_eq!(short_f64_of_f32(-2.7), -2.7);
    assert_eq!(short_f64_of_f32(0.5), 0.5);
    assert_eq!(short_f64_of_f32(1e30), 1e30);
    assert_eq!(short_f64_of_f32(0.0).to_bits(), 0.0f64.to_bits());
    assert_eq!(short_f64_of_f32(-0.0).to_bits(), (-0.0f64).to_bits());
    assert_eq!(short_f64_of_f32(f32::INFINITY), f64::INFINITY);
    assert_eq!(short_f64_of_f32(f32::NEG_INFINITY), f64::NEG_INFINITY);
    assert!(short_f64_of_f32(f32::NAN).is_nan());
}

#[test]
fn short_f64_of_f32_converts_back_to_the_same_f32() {
    for value in [
        1.1f32,
        0.3,
        -2.7,
        0.1,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::MAX,
        f32::MIN,
        f32::EPSILON,
        16777216.0,
        123456.79,
        7.038531e-26,
        -7.038531e-26,
    ] {
        assert_eq!((short_f64_of_f32(value) as f32).to_bits(), value.to_bits(), "{value}");
    }
    for bits in (0..=u32::MAX).step_by(4099) {
        let value = f32::from_bits(bits);
        if value.is_finite() {
            assert_eq!((short_f64_of_f32(value) as f32).to_bits(), bits, "{value}");
        }
    }
}

#[test]
fn with_short_floats_shortens_f32s_widened_by_json_and_to_value() -> anyhow::Result<()> {
    let widened = serde_json::to_value(Vec3::new(1.1, 0.3, -2.7))?;
    assert_eq!(widened.to_string(), "[1.100000023841858,0.30000001192092896,-2.700000047683716]");
    assert_eq!(with_short_floats(widened).to_string(), "[1.1,0.3,-2.7]");

    let nested = json!({
        "position": { "x": 1.1f32, "y": [0.3f32, [-2.7f32]] },
        "children": [{ "scale": 0.1f32 }],
    });
    assert_eq!(
        with_short_floats(nested),
        json!({
            "position": { "x": 1.1, "y": [0.3, [-2.7]] },
            "children": [{ "scale": 0.1 }],
        })
    );
    Ok(())
}

#[test]
fn with_short_floats_leaves_f64s_integers_and_other_values_alone() {
    let value = json!({
        "f64": 0.1f64,
        "integers": [0, 7, -3, u64::MAX, i64::MIN],
        "whole_float": 2.0,
        "other": [null, true, "1.100000023841858"],
    });
    let shortened = with_short_floats(value.clone());
    assert_eq!(shortened, value);
    assert_eq!(shortened["f64"].as_f64(), Some(0.1f64));
    assert!(shortened["integers"].as_array().unwrap().iter().all(|integer| !integer.is_f64()));
    assert!(shortened["whole_float"].is_f64());
    assert_eq!(shortened["other"][2], Value::String("1.100000023841858".to_string()));
}
