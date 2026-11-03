use serde_json::{Number, Value};

// Rust stores positions, sizes and most other numbers as f32, but scripts and JSON read every number as an f64.
// Widening an f32 keeps its exact binary value, which reads as noise: 1.1 becomes 1.100000023841858. So an f32
// leaves Rust as the f64 of its shortest decimal (1.1).

// Any Rust number as the f64 a script sees.
pub trait ToF64 {
    fn to_f64(self) -> f64;
}
macro_rules! impl_ToF64 {
    ($($t:ty),+) => {
        $(impl ToF64 for $t {
            fn to_f64(self) -> f64 {
                self as f64
            }
        })+
    };
}
impl_ToF64!(f64, i64, i32, i16, i8, u64, u32, u16, u8, usize, isize);
impl ToF64 for f32 {
    fn to_f64(self) -> f64 {
        short_f64_of_f32(self)
    }
}

// `to_string` writes an f32's shortest decimal. Rarely, that decimal's nearest f64 rounds back to a different f32
// (7.038531e-26 is one), and then the exact value is kept instead.
pub fn short_f64_of_f32(value: f32) -> f64 {
    let short: f64 = value.to_string().parse().unwrap_or(f64::from(value));
    if short as f32 == value { short } else { f64::from(value) }
}

// serde_json keeps every number as an f64, so JSON built from f32 fields holds them widened. Every number that is
// exactly an f32 goes back to its shortest decimal.
pub fn with_short_floats(mut value: Value) -> Value {
    shorten_floats(&mut value);
    value
}

fn shorten_floats(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64().filter(|float| number.is_f64() && f64::from(*float as f32) == *float)
                && let Some(short) = Number::from_f64(short_f64_of_f32(float as f32))
            {
                *number = short;
            }
        }
        Value::Array(values) => values.iter_mut().for_each(shorten_floats),
        Value::Object(fields) => fields.values_mut().for_each(shorten_floats),
        _ => {}
    }
}
