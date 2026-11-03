import { denoOps } from "./object_types/__globalOps__"
import { ObjectId } from "./object_types/shared"

/** This service exposes methods for controlling Clock objects on SkAnimator, PropertyAnimator, and SoundPlayer. */
export type ClockService = {
  /** */
  pause: (clockId: ObjectId) => void
  /** */
  play: (clockId: ObjectId, animationTimeSecs: number) => void
  /** */
  setSpeed: (clockId: ObjectId, speed: number) => void
}

export const clockService: ClockService = {
  pause: (clockId: ObjectId) => {
    denoOps.__pauseClock__(clockId)
  },
  play: (clockId: ObjectId, animationTimeSecs: number) => {
    denoOps.__playClock__(clockId, animationTimeSecs)
  },
  setSpeed: (clockId: ObjectId, speed: number) => {
    denoOps.__setClockSpeed__(clockId, speed)
  },
}
