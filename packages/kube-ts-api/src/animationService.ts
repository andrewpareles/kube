import { denoOps } from "./object_types/__globalOps__"
import { ObjectId } from "./object_types/shared"
import type { InterpolationStyle } from "./object_types/types"

/** This service exposes methods for smoothly transitioning animation values. */
export type AnimationService = {
  /** Use this to smoothly change the weight of a SkAnimator or PropertyAnimator. 
  */
  smoothlyChangeWeight: (animatorId: ObjectId, targetWeight: number, durationMs: number, interpolationStyle?: InterpolationStyle) => void
  /**
  * Use this any time you want to smoothly move one of an object's numbers to a new value, like opening a door when a button is pressed. For example, `tweenTo(doorId, ["location", "rotation", "y"], 90, 500)`.
  *
  * The number moves from its current value to targetValue over durationMs, and then stays there. Tweening a number that is already tweening starts from wherever it has got to.
  * While a tween plays, it wins over any PropertyAnimator on the same number, and it overwrites anything a script writes to that number.
  *
  * This returns the Tween object. It sits at the world root while it plays and deletes itself when it finishes. Delete it yourself to stop the tween where it is.
  * */
  tweenTo: (objectId: ObjectId, propertyPath: string[], targetValue: number, durationMs: number, interpolationStyle?: InterpolationStyle) => ObjectId
}

export const animationService: AnimationService = {
  smoothlyChangeWeight: (animatorId: ObjectId, targetWeight: number, durationMs: number, interpolationStyle?: InterpolationStyle) => {
    denoOps.__smoothlyChangeWeight__(animatorId, targetWeight, durationMs, interpolationStyle ?? null)
  },
  tweenTo: (objectId: ObjectId, propertyPath: string[], targetValue: number, durationMs: number, interpolationStyle?: InterpolationStyle) => {
    // read the number the same way a script would, so a rotation reads back as the degrees last written to it
    let currentValue: unknown = denoOps.__getRustObject__(objectId)
    for (const field of propertyPath) {
      if (currentValue === null || typeof currentValue !== 'object') break
      currentValue = Reflect.get(currentValue, field)
    }
    if (typeof currentValue !== 'number') throw new Error(`tweenTo: ${JSON.stringify(propertyPath)} is not a number on object ${objectId}`)
    return denoOps.__tweenTo__(objectId, JSON.stringify(propertyPath), currentValue, targetValue, durationMs, interpolationStyle ?? null)
  },
}
