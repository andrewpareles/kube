'use client';

// Walk animation is driven fully client-side: this script fades every character's walk animator
// in and out from its velocity across the ground, so moving platforms do not animate idle players
// and nothing goes through the server.

// Start walking above this speed and stop below the lower one: the gap (hysteresis) sits well
// above idle physics noise and well below real walking speed, so a speed hovering near one
// threshold can't flap the state. WALK_FULL_SPEED is where the walk cycle plays at its authored
// pace, and WALK_FADE_MS is the idle<->walk crossfade time.
const WALK_START_SPEED = 1.0;
const WALK_STOP_SPEED = 0.6;
const WALK_FULL_SPEED = 9;
const WALK_FADE_MS = 150;
// Per-tick smoothing for the body speed: a spammed WASD key reads as sustained movement instead
// of rapid start/stop flips that keep restarting the crossfade before it can show.
const SPEED_SMOOTHING_ALPHA = 0.2;

// Airborne detection uses game.objectService.getGroundSupport, whose defaults are the engine's 1cm reach and
// 55 degree slope limit — the exact walkable-support test the engine uses to gate player jumping. Becoming airborne must hold for a few consecutive ticks
// (stair edges must not flash the fall pose in) while landing registers immediately. Each fade
// then runs to COMPLETION before the state may touch the animator again: the landing fade-out
// always finishes before the next jump's fade-in begins, so no fade ever restarts mid-curve —
// which is what used to park the arms mid-swing. The fade-out is quicker than the swing-in so
// the arms settle promptly on landing.
const STATE_DELAY_TICKS = 4;
const JUMP_FADE_IN_MS = 300;
const JUMP_FADE_OUT_MS = 150;

let localCharacterBodyId = null;
const localWalkingState = createState(false);

export async function setLocalCharacterFromServer(bodyObjectId) {
  if (typeof bodyObjectId !== "number") return;
  localCharacterBodyId = bodyObjectId;
}

const WalkingHud = () => (
  <div className="absolute top-4 right-4 px-4 py-3 rounded-md bg-black/70 text-white text-lg">
    walking: {String(localWalkingState.get())}
  </div>
);
game.guiService.show("walking_hud", WalkingHud);

// characterBodyId -> { walkClockId, walkAnimatorId, jumpAnimatorId, isWalking, smoothedSpeed,
//                      appliedSpeedMultiplier, isAirborne, appliedAirborne, airborneTicks, fadeEndMs }
const walkStateOfBodyId = new Map();

const findWalkKit = (body) => {
  const characterRoot = body.parent;
  const walkClock = characterRoot?.getChild("Walk-Clock");
  const walkAnimator = characterRoot?.getChild("Walk-Animator");
  if (!walkClock || !walkAnimator) return null;
  const jumpAnimator = characterRoot?.getChild("Jump-Animator");
  return { walkClockId: walkClock.id, walkAnimatorId: walkAnimator.id, jumpAnimatorId: jumpAnimator?.id ?? null };
};

// The drive loop lives in an async function instead of at the top level: exported functions
// only get registered after the module body finishes, so it must not block on an infinite loop.
const driveWalkAnimations = async () => {
  while (true) {
    const seenBodyIds = new Set();
    for (const player of game.objectService.getObjectsByType("player")) {
      if (player.type !== "player" || player.characterObjectId === null) continue;
      const body = game.objectService.getObjectById(player.characterObjectId);
      if (!body || !("movement" in body)) continue;
      seenBodyIds.add(body.id);

      let walkState = walkStateOfBodyId.get(body.id);
      if (!walkState) {
        const walkKit = findWalkKit(body);
        if (!walkKit) continue;
        walkState = { ...walkKit, isWalking: false, smoothedSpeed: 0, appliedSpeedMultiplier: 1, isAirborne: false, appliedAirborne: false, airborneTicks: 0, fadeEndMs: 0 };
        walkStateOfBodyId.set(body.id, walkState);
      }

      const velocity = game.objectService.getVelocityRelativeToGround(body.id);
      const horizontalSpeed = Math.sqrt(velocity.x * velocity.x + velocity.z * velocity.z);
      walkState.smoothedSpeed += (horizontalSpeed - walkState.smoothedSpeed) * SPEED_SMOOTHING_ALPHA;

      const walkThreshold = walkState.isWalking ? WALK_STOP_SPEED : WALK_START_SPEED;
      const isWalking = walkState.smoothedSpeed > walkThreshold;
      if (isWalking !== walkState.isWalking) {
        walkState.isWalking = isWalking;
        game.animationService.smoothlyChangeWeight(walkState.walkAnimatorId, isWalking ? 100 : 0, WALK_FADE_MS);
      }
      if (body.id === localCharacterBodyId && localWalkingState.get() !== isWalking) {
        localWalkingState.set(isWalking);
      }
      if (isWalking) {
        // Only touch the clock when the speed changed noticeably, instead of rewriting it every tick.
        const speedMultiplier = walkState.smoothedSpeed / WALK_FULL_SPEED;
        if (Math.abs(speedMultiplier - walkState.appliedSpeedMultiplier) > 0.05) {
          walkState.appliedSpeedMultiplier = speedMultiplier;
          game.clockService.setSpeed(walkState.walkClockId, speedMultiplier);
        }
      }

      if (walkState.jumpAnimatorId !== null) {
        const rawAirborne = game.objectService.getGroundSupport(body.id) === null;
        if (rawAirborne) {
          walkState.airborneTicks += 1;
          if (walkState.airborneTicks > STATE_DELAY_TICKS) walkState.isAirborne = true;
        } else {
          walkState.airborneTicks = 0;
          walkState.isAirborne = false;
        }
        // Apply the state to the animator only once the previous fade has fully finished, so the
        // fall pose always completes its swing in each direction before reversing.
        if (walkState.isAirborne !== walkState.appliedAirborne && getTime() >= walkState.fadeEndMs) {
          walkState.appliedAirborne = walkState.isAirborne;
          const fadeMs = walkState.isAirborne ? JUMP_FADE_IN_MS : JUMP_FADE_OUT_MS;
          walkState.fadeEndMs = getTime() + fadeMs;
          game.animationService.smoothlyChangeWeight(walkState.jumpAnimatorId, walkState.isAirborne ? 100 : 0, fadeMs);
        }
      }
    }
    for (const bodyId of [...walkStateOfBodyId.keys()]) {
      if (!seenBodyIds.has(bodyId)) walkStateOfBodyId.delete(bodyId);
    }
    await tick();
  }
};
void driveWalkAnimations();
