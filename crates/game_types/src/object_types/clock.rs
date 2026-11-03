use serde::{Deserialize, Serialize};

use super::script::ObjectLocation;

// INVARIANT: if clock cannot be read then:
// - give error if changing it or creating it
// - give t=0 if reading it (WeightTransition), ie treat it as 100% played and done (unexpected for this to ever happen)

fn multiplexClockMs(timeLocation: ObjectLocation, server_ms: f64, client_ms: Option<f64>) -> anyhow::Result<f64> {
    match timeLocation {
        ObjectLocation::Server => Ok(server_ms),
        ObjectLocation::Client => client_ms.ok_or_else(|| anyhow::anyhow!("a client clock cannot be read from the server")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClockState {
    Paused {
        animationTimeSecs: f32, // the time in the animation.
    },
    Playing {
        localStartMs: f64, // the simulation time at which the clock started, either locally on this client, or on the server (which the client estimates)
        timeLocation: ObjectLocation, // whether localStartMs is client time or estimated server time.
    },
}
impl ClockState {
    pub fn new_paused(animationTimeSecs: f32) -> anyhow::Result<Self> {
        if !animationTimeSecs.is_finite() {
            return Err(anyhow::anyhow!("ClockState Paused animationTimeSecs must be finite"));
        }
        Ok(Self::Paused { animationTimeSecs })
    }

    pub fn new_playing(localStartMs: f64, timeLocation: ObjectLocation) -> anyhow::Result<Self> {
        if !localStartMs.is_finite() {
            return Err(anyhow::anyhow!("ClockState Playing localStartMs must be finite"));
        }
        Ok(Self::Playing { localStartMs, timeLocation })
    }
    pub fn new_clock_state(
        animation_time_secs: f32,
        speed: f32,
        time_location: ObjectLocation,
        server_ms: f64,
        client_ms: Option<f64>,
    ) -> anyhow::Result<ClockState> {
        let time_ms = multiplexClockMs(time_location, server_ms, client_ms)?;
        let clock_state = if speed.abs() <= f32::EPSILON {
            ClockState::new_paused(animation_time_secs)?
        } else {
            let animation_time_ms = animation_time_secs * 1000.0;
            let local_start_ms = time_ms - (animation_time_ms / speed) as f64;
            ClockState::new_playing(local_start_ms, time_location)?
        };
        Ok(clock_state)
    }

    pub fn readRawTimeMs(&self, speed: f32, server_ms: f64, client_ms: Option<f64>) -> f64 {
        match self {
            Self::Paused { animationTimeSecs } => *animationTimeSecs as f64 * 1000.0,
            Self::Playing { localStartMs, timeLocation } => {
                let Ok(time_ms) = multiplexClockMs(*timeLocation, server_ms, client_ms) else {
                    // treat animation as if it's as t=0
                    return 0.0;
                };
                (time_ms - *localStartMs) * speed as f64
            }
        }
    }

    pub fn readTimeSecs(&self, speed: f32, should_loop: bool, duration_secs: f32, server_ms: f64, client_ms: Option<f64>) -> f32 {
        let time_ms_nonclamped = self.readRawTimeMs(speed, server_ms, client_ms);
        let time_secs_nonclamped = time_ms_nonclamped / 1000.0;

        let time_secs = if duration_secs <= f32::EPSILON {
            0.0
        } else if should_loop {
            time_secs_nonclamped.rem_euclid(duration_secs as f64)
        } else {
            time_secs_nonclamped.clamp(0.0, duration_secs as f64)
        };

        time_secs as f32
    }
}

// Clock state can be changed by server or client scripts. Client-side changes stay local and may desync.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClockInfo {
    pub clockState: ClockState,
    pub shouldLoop: bool, // loop forever
    pub speed: f32,       // speed multiplier
}
impl ClockInfo {
    // Native scripts and browser controls use the same clock edits, then store the result in their own scene.
    pub fn withPausedClock(&self, server_ms: f64, client_ms: Option<f64>) -> anyhow::Result<Self> {
        let animation_time_secs = (self.clockState.readRawTimeMs(self.speed, server_ms, client_ms) / 1000.0) as f32;
        Ok(self.withNewClockState(ClockState::new_paused(animation_time_secs)?))
    }

    pub fn withPlayingClock(&self, animation_time_secs: f32, time_location: ObjectLocation, server_ms: f64, client_ms: Option<f64>) -> anyhow::Result<Self> {
        Ok(self.withNewClockState(ClockState::new_clock_state(
            animation_time_secs,
            self.speed,
            time_location,
            server_ms,
            client_ms,
        )?))
    }

    // preserves elapsed time while changing speed (aka without skipping)
    pub fn withNewSpeed(&self, new_speed: f32, server_ms: f64, client_ms: Option<f64>) -> anyhow::Result<Self> {
        match self.clockState {
            // if it's paused, keep it paused and simply adjust the speed
            ClockState::Paused { animationTimeSecs } => {
                return Ok(Self {
                    speed: new_speed,
                    clockState: ClockState::new_paused(animationTimeSecs)?,
                    shouldLoop: self.shouldLoop,
                });
            }
            // if it's playing, adjust localStartMs too
            ClockState::Playing { timeLocation, .. } => {
                let time_ms = multiplexClockMs(timeLocation, server_ms, client_ms)?;
                let elapsed_ms = self.clockState.readRawTimeMs(self.speed, server_ms, client_ms);
                // if the speed is 0, pause it
                if new_speed.abs() <= f32::EPSILON {
                    return Ok(Self {
                        speed: new_speed,
                        clockState: ClockState::new_paused((elapsed_ms / 1000.0) as f32)?,
                        shouldLoop: self.shouldLoop,
                    });
                } else {
                    // play it with the adjusted speed/time
                    let newLocalStartMs = time_ms - elapsed_ms / new_speed as f64;
                    return Ok(Self {
                        speed: new_speed,
                        clockState: ClockState::new_playing(newLocalStartMs, timeLocation)?,
                        shouldLoop: self.shouldLoop,
                    });
                }
            }
        };
    }

    fn withNewClockState(&self, new_clock_state: ClockState) -> Self {
        Self {
            clockState: new_clock_state,
            speed: self.speed,
            shouldLoop: self.shouldLoop,
        }
    }
}

// ----------------------- Weight transitions - shared code -----------------------

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum InterpolationStyle {
    Step,
    Linear,
    EaseInOut,
    EaseIn,
    EaseOut,
}
impl InterpolationStyle {
    pub const DEFAULT: Self = Self::EaseInOut;

    pub fn asStr(&self) -> &'static str {
        match self {
            Self::Step => "Step",
            Self::Linear => "Linear",
            Self::EaseInOut => "EaseInOut",
            Self::EaseIn => "EaseIn",
            Self::EaseOut => "EaseOut",
        }
    }

    pub fn fromString(value: &String) -> Option<Self> {
        match value.as_str() {
            "Step" => Some(Self::Step),
            "Linear" => Some(Self::Linear),
            "EaseInOut" => Some(Self::EaseInOut),
            "EaseIn" => Some(Self::EaseIn),
            "EaseOut" => Some(Self::EaseOut),
            _ => None,
        }
    }

    fn computeLinearAlpha(&self, linear_alpha: f32) -> f32 {
        let alpha = linear_alpha.clamp(0.0, 1.0);
        match self {
            Self::Step => 0.0,
            Self::Linear => alpha,
            Self::EaseIn => alpha * alpha,
            Self::EaseOut => 1.0 - (1.0 - alpha) * (1.0 - alpha),
            Self::EaseInOut => {
                if alpha < 0.5 {
                    2.0 * alpha * alpha
                } else {
                    1.0 - ((-2.0 * alpha + 2.0) * (-2.0 * alpha + 2.0)) / 2.0
                }
            }
        }
    }

    pub fn computeAlpha(&self, t0: f32, t1: f32, sample_time: f32) -> f32 {
        let dt = t1 - t0;
        if dt <= f32::EPSILON {
            return 0.0;
        }
        let linear_alpha = ((sample_time - t0) / dt).clamp(0.0, 1.0);
        self.computeLinearAlpha(linear_alpha)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WeightTransition {
    pub startedAtMs: f64,
    pub timeLocation: ObjectLocation,

    pub initValue: f32,
    pub finalValue: f32,
    pub durationMs: f32,

    pub easing: InterpolationStyle,
}

impl WeightTransition {
    // Retarget from the displayed weight, including an unfinished transition, so interrupted fades remain continuous.
    pub fn fromCurrentWeight(
        timeLocation: ObjectLocation,
        server_ms: f64,
        client_ms: Option<f64>,

        weight: f32,
        weight_transition: Option<Self>,
        finalValue: f32,
        durationMs: f32,
        easing: InterpolationStyle,
    ) -> anyhow::Result<Self> {
        let startedAtMs = multiplexClockMs(timeLocation, server_ms, client_ms)?;
        let initValue = Self::readValueWithFallback(weight_transition, weight, server_ms, client_ms);
        Ok(Self {
            startedAtMs,
            timeLocation,
            initValue,
            finalValue,
            durationMs,
            easing,
        })
    }

    pub fn isFinished(&self, server_ms: f64, client_ms: Option<f64>) -> bool {
        // trivially the animation is treated as done
        let Ok(time_ms) = multiplexClockMs(self.timeLocation, server_ms, client_ms) else {
            return true;
        };
        // actual computation
        let is_finished = time_ms - self.startedAtMs >= self.durationMs as f64;
        is_finished
    }

    fn readValue(&self, server_ms: f64, client_ms: Option<f64>) -> f32 {
        // trivially the animation is treated as done
        let Ok(time_ms) = multiplexClockMs(self.timeLocation, server_ms, client_ms) else {
            return self.finalValue;
        };

        // actual computation
        let is_finished = self.isFinished(server_ms, client_ms);

        let value = if is_finished {
            // this check is mainly here for step easing, and also zero-duration animations
            self.finalValue
        } else {
            let linear_alpha = ((time_ms - self.startedAtMs) as f32 / self.durationMs).clamp(0.0, 1.0);
            let alpha = self.easing.computeLinearAlpha(linear_alpha);
            self.initValue + (self.finalValue - self.initValue) * alpha
        };

        value
    }

    pub fn readValueWithFallback(weight_transition: Option<Self>, base_value: f32, server_ms: f64, client_ms: Option<f64>) -> f32 {
        let Some(weight_transition) = weight_transition else {
            return base_value;
        };
        let value = weight_transition.readValue(server_ms, client_ms);
        value
    }
}
