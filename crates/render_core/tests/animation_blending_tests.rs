use render_core::computeBlendedWeights::{AnimationBlendSample, computeBlendedWeights};

fn assert_close(actual: f32, expected: f32) {
    let difference = (actual - expected).abs();
    assert!(difference <= 1e-5, "actual={actual}, expected={expected}, difference={difference}");
}

#[test]
fn compute_animation_blend_weights_gives_higher_priority_first_claim() {
    let blend_weights = computeBlendedWeights(&[
        AnimationBlendSample {
            priority: 3,
            computed_weight: 40.0,
            is_valid: true,
        },
        AnimationBlendSample {
            priority: 8,
            computed_weight: 75.0,
            is_valid: true,
        },
        AnimationBlendSample {
            priority: 3,
            computed_weight: 40.0,
            is_valid: true,
        },
    ]);

    assert_close(blend_weights.sample_weights[0], 0.125);
    assert_close(blend_weights.sample_weights[1], 0.75);
    assert_close(blend_weights.sample_weights[2], 0.125);
    assert_close(blend_weights.total_animation_weight, 1.0);
    assert_close(blend_weights.remaining_weight, 0.0);
}

#[test]
fn compute_animation_blend_weights_ignores_invalid_and_nonpositive_samples() {
    let blend_weights = computeBlendedWeights(&[
        AnimationBlendSample {
            priority: 10,
            computed_weight: 100.0,
            is_valid: false,
        },
        AnimationBlendSample {
            priority: 8,
            computed_weight: 0.0,
            is_valid: true,
        },
        AnimationBlendSample {
            priority: 1,
            computed_weight: 25.0,
            is_valid: true,
        },
    ]);

    assert_close(blend_weights.sample_weights[0], 0.0);
    assert_close(blend_weights.sample_weights[1], 0.0);
    assert_close(blend_weights.sample_weights[2], 0.25);
    assert_close(blend_weights.total_animation_weight, 0.25);
    assert_close(blend_weights.remaining_weight, 0.75);
}

#[test]
fn compute_animation_blend_weights_scales_same_priority_group() {
    let blend_weights = computeBlendedWeights(&[
        AnimationBlendSample {
            priority: 5,
            computed_weight: 80.0,
            is_valid: true,
        },
        AnimationBlendSample {
            priority: 5,
            computed_weight: 80.0,
            is_valid: true,
        },
    ]);

    assert_close(blend_weights.sample_weights[0], 0.5);
    assert_close(blend_weights.sample_weights[1], 0.5);
    assert_close(blend_weights.total_animation_weight, 1.0);
    assert_close(blend_weights.remaining_weight, 0.0);
}
