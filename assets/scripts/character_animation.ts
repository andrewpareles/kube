'use client'
// Plays this character's idle, walk and jump animations to match how it moves.

const WALK_SPEED = 7;
const MIN_WALK_SPEED = 1;
const MIN_WALK_CLOCK_SPEED = 0.5;
const GROUND_DISTANCE = 0.2;
const STATE_LEEWAY_MS = 100;
const FADE_MS = 150;
const CHECK_MS = 50;

type AnimationState = "idle" | "walk" | "jump";
const ANIMATOR_NAME_OF_STATE: { [state in AnimationState]: string } = { idle: "IdleAnimator", walk: "WalkAnimator", jump: "JumpAnimator" };

let state: AnimationState = "idle";
let nextState: AnimationState = "idle";
let nextStateSinceMs = getTime();
let walkClockSpeed = 1;

setInterval(() => {
  // A new player's character can reach this client a piece at a time, so its parts are looked up once they are all here.
  const character = script.parent;
  const body = character?.getChild("Body") ?? null;
  const walkAnimator = character?.getChild(ANIMATOR_NAME_OF_STATE.walk) ?? null;
  if (character === null || body === null || walkAnimator?.type !== "skAnimator") return;

  const ground = game.objectService.getGroundSupport(body.id, { maxDistance: GROUND_DISTANCE });
  const velocity = game.objectService.getVelocityRelativeToGround(body.id);
  const speed = Math.hypot(velocity.x, velocity.z);
  const wantedState: AnimationState = ground === null ? "jump" : speed > MIN_WALK_SPEED ? "walk" : "idle";

  const wantedWalkClockSpeed = Math.max(speed / WALK_SPEED, MIN_WALK_CLOCK_SPEED);
  if (walkAnimator.clockId !== null && Math.abs(wantedWalkClockSpeed - walkClockSpeed) > 0.05) {
    walkClockSpeed = wantedWalkClockSpeed;
    game.clockService.setSpeed(walkAnimator.clockId, walkClockSpeed);
  }

  if (wantedState !== nextState) {
    nextState = wantedState;
    nextStateSinceMs = getTime();
  }
  if (nextState === state || getTime() - nextStateSinceMs < STATE_LEEWAY_MS) return;
  state = nextState;
  for (const [animatorState, animatorName] of Object.entries(ANIMATOR_NAME_OF_STATE)) {
    const animator = character.getChild(animatorName);
    if (animator !== null) game.animationService.smoothlyChangeWeight(animator.id, animatorState === state ? 100 : 0, FADE_MS);
  }
}, CHECK_MS);
