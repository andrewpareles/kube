use game_types::consts::ANIMATOR_WEIGHT_MAX;

// --------------------------- blending logic ---------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationBlendSample {
    pub priority: u8,
    pub computed_weight: f32,
    pub is_valid: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationBlendWeights {
    pub sample_weights: Vec<f32>,
    pub total_animation_weight: f32,
    pub remaining_weight: f32,
}

// this is a helper for computePropertyValue and computeSkeletonPose.
// takes in N weights in order of priority, returns N weights to actually use.
pub fn computeBlendedWeights(animation_samples: &[AnimationBlendSample]) -> AnimationBlendWeights {
    let mut priorities: Vec<u8> = Vec::new();
    for sample in animation_samples {
        if !sample.is_valid || sample.computed_weight <= 0.0 {
            continue;
        }
        if !priorities.contains(&sample.priority) {
            priorities.push(sample.priority);
        }
    }
    priorities.sort_by(|a, b| b.cmp(a));

    let mut sample_weights = vec![0.0; animation_samples.len()];
    let mut total_animation_weight = 0.0;
    for priority in priorities {
        let sum_priority_weight = animation_samples
            .iter()
            .filter(|sample| sample.is_valid && sample.computed_weight > 0.0 && sample.priority == priority)
            .map(|sample| sample.computed_weight / ANIMATOR_WEIGHT_MAX)
            .sum::<f32>();
        if sum_priority_weight <= 0.0 {
            continue;
        }

        let remaining_weight = 1.0 - total_animation_weight;
        if remaining_weight <= 0.0 {
            break;
        }

        let group_scale = if sum_priority_weight > remaining_weight {
            remaining_weight / sum_priority_weight
        } else {
            1.0
        };

        for (sample_idx, sample) in animation_samples.iter().enumerate() {
            if !sample.is_valid || sample.computed_weight <= 0.0 || sample.priority != priority {
                continue;
            }
            sample_weights[sample_idx] = (sample.computed_weight / ANIMATOR_WEIGHT_MAX) * group_scale;
        }

        total_animation_weight += sum_priority_weight.min(remaining_weight);
        if total_animation_weight >= 1.0 {
            break;
        }
    }

    AnimationBlendWeights {
        sample_weights,
        total_animation_weight,
        remaining_weight: (1.0 - total_animation_weight).max(0.0),
    }
}
