'use client'
// The default character controller. It moves the local player's character. Change the numbers below to tune it, or delete
// this script to write your own. A server script can change one player's movement with setMovement(player.id, { speed: 15 }).
// Shift toggles shift lock: the mouse turns the camera, the cursor becomes a crosshair, the character faces where the camera
// looks, and the camera moves over the character's right shoulder.

type Vec3 = { x: number, y: number, z: number };

const movementSettings = { speed: 7, acceleration: 160, jumpHeight: 3, stepHeight: 0.1 };

const JUMP_RESET_MS = 300;
const AIR_JUMP_MS = 100;
const SNAP_DOWN_HEIGHT = 0.1;
const MAX_GROUND_SLOPE_DEGREES = 55;
const FACING_NATURAL_FREQUENCY = 11;
const FACING_DAMPING_RATIO = 1;
const FACING_MAX_ANGULAR_ACCELERATION = 700;
const SHIFT_LOCK_TURN_SECS = 0.04;
const SHIFT_LOCK_CAMERA_OFFSET: Vec3 = { x: 1.5, y: 0, z: 0 };
const SHIFT_LOCK_FULL_OFFSET_ZOOM = 3; // TODO: AI-PICKED-VALUE: zoomed in closer than this, shift lock's offset shrinks to nothing at zoom 0, so a camera zoomed all the way in sits in the head. It matches zoom_fade's FADE_START_ZOOM, where the character is fully back.
const UPRIGHT_NATURAL_FREQUENCY = 34;
const UPRIGHT_DAMPING_RATIO = 1;
const UPRIGHT_MAX_ANGULAR_ACCELERATION = 3600;

const RADIANS_PER_DEGREE = Math.PI / 180;
const ZERO: Vec3 = { x: 0, y: 0, z: 0 };
const UP: Vec3 = { x: 0, y: 1, z: 0 };
const DOWN: Vec3 = { x: 0, y: -1, z: 0 };

type JumpState =
  | { kind: "groundedReady" }
  | { kind: "waitingToGetOffGround", jumpStartMs: number }
  | { kind: "offGroundNeverJumped", leftGroundMs: number }
  | { kind: "waitingToLand" };

let nowMs = 0;
let jumpState: JumpState = { kind: "groundedReady" };
let autostep: { startPosition: Vec3, translation: Vec3, stepHeight: number } | null = null;
let facingSpinRate = 0;
let isShiftLocked = false;

const wrapAngle = (angle: number) => {
  const wrapped = (angle + Math.PI) % (2 * Math.PI);
  return (wrapped < 0 ? wrapped + 2 * Math.PI : wrapped) - Math.PI;
};

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

const copyVec3 = (v: Vec3): Vec3 => ({ x: v.x, y: v.y, z: v.z });

const getPhysicalObject = (objectId: number) => {
  const object = game.objectService.getObjectById(objectId);
  if (object === null || !("movement" in object)) return null;
  return object;
};

export const setMovement = async (_playerObjectId: number, changes: Partial<typeof movementSettings>) => {
  Object.assign(movementSettings, changes);
};

const getMovementSettings = () => {
  const { speed, acceleration, jumpHeight, stepHeight } = movementSettings;
  return { speed, acceleration: Math.max(acceleration, 0), jumpHeight: Math.max(jumpHeight, 0), stepHeight: Math.max(stepHeight, 0) };
};

const getGroundSpinRate = (groundObjectId: number) => {
  const groundObject = getPhysicalObject(groundObjectId);
  return groundObject === null ? 0 : groundObject.movement.angularVelocity.y * RADIANS_PER_DEGREE;
};

const shouldJump = (isGrounded: boolean, isJumpHeld: boolean): boolean => {
  switch (jumpState.kind) {
    case "groundedReady":
      if (isJumpHeld && isGrounded) {
        jumpState = { kind: "waitingToGetOffGround", jumpStartMs: nowMs };
        return true;
      }
      if (!isGrounded) jumpState = { kind: "offGroundNeverJumped", leftGroundMs: nowMs };
      return false;
    case "waitingToGetOffGround":
      if (!isGrounded) jumpState = { kind: "waitingToLand" };
      else if (nowMs > jumpState.jumpStartMs + JUMP_RESET_MS) jumpState = { kind: "groundedReady" };
      return false;
    case "offGroundNeverJumped":
      if (isGrounded) {
        jumpState = { kind: "groundedReady" };
        return false;
      }
      if (nowMs <= jumpState.leftGroundMs + AIR_JUMP_MS) {
        if (!isJumpHeld) return false;
        jumpState = { kind: "waitingToLand" };
        return true;
      }
      jumpState = { kind: "waitingToLand" };
      return false;
    case "waitingToLand":
      if (isGrounded) jumpState = { kind: "groundedReady" };
      return false;
  }
};

const getStepTranslation = (characterObjectId: number, forward: Vec3, stepHeight: number): Vec3 | null => {
  const forwardDistance = vec3.length(forward);
  if (forwardDistance * forwardDistance <= 1e-7) return null;
  const forwardDirection = vec3.normalize(forward);
  const minGroundNormalY = Math.cos(MAX_GROUND_SLOPE_DEGREES * RADIANS_PER_DEGREE);

  const wall = game.objectService.castObjectShape(characterObjectId, forwardDirection, { maxDistance: forwardDistance });
  if (wall === null || wall.normal.y >= minGroundNormalY) return null;

  const stepUp = { x: 0, y: stepHeight, z: 0 };
  if (game.objectService.castObjectShape(characterObjectId, UP, { maxDistance: stepHeight }) !== null) return null;
  if (game.objectService.castObjectShape(characterObjectId, forwardDirection, { maxDistance: forwardDistance, startOffset: stepUp }) !== null) return null;

  const stepDown = game.objectService.castObjectShape(characterObjectId, DOWN, { maxDistance: stepHeight, startOffset: vec3.add(stepUp, forward) });
  if (stepDown === null || stepDown.normal.y < minGroundNormalY) return null;

  return vec3.sub(vec3.add(stepUp, forward), { x: 0, y: stepDown.distance, z: 0 });
};

game.clientInputService.addKeyDownListener((key) => {
  if (key !== "ShiftLeft" && key !== "ShiftRight") return;
  isShiftLocked = !isShiftLocked;
  if (isShiftLocked) game.clientInputService.lockMouse();
  else game.clientInputService.unlockMouse();
  game.clientInputService.setCameraOffset(isShiftLocked ? SHIFT_LOCK_CAMERA_OFFSET : ZERO);
  game.clientInputService.setMouseCursor(isShiftLocked ? "crosshair" : "default");
});

game.clientCharacterService.addBeforeStepListener((characterObjectId, dtSecs) => {
  nowMs += dtSecs * 1000;
  autostep = null;
  // This follows the zoom the player picked, not getZoom(), because a wall that pulls the camera in would shrink the offset, which frees
  // the camera, which grows the offset again.
  if (isShiftLocked) game.clientInputService.setCameraOffset(vec3.scale(SHIFT_LOCK_CAMERA_OFFSET, Math.min(game.clientInputService.getDesiredZoom() / SHIFT_LOCK_FULL_OFFSET_ZOOM, 1)));
  const character = getPhysicalObject(characterObjectId);
  const settings = getMovementSettings();
  if (character === null) return;
  const movement = character.movement;
  const velocity = copyVec3(movement.velocity);
  const angularVelocity = vec3.scale(copyVec3(movement.angularVelocity), RADIANS_PER_DEGREE);
  const ground = game.objectService.getGroundSupport(characterObjectId);

  const camera = game.clientInputService.getCameraDirection();
  const forward = vec3.normalize({ x: camera.x, y: 0, z: camera.z });
  const right = { x: -forward.z, y: 0, z: forward.x };
  let moveDirection = ZERO;
  if (game.clientInputService.isKeyDown("W") || game.clientInputService.isKeyDown("ArrowUp")) moveDirection = vec3.add(moveDirection, forward);
  if (game.clientInputService.isKeyDown("S") || game.clientInputService.isKeyDown("ArrowDown")) moveDirection = vec3.sub(moveDirection, forward);
  if (game.clientInputService.isKeyDown("D") || game.clientInputService.isKeyDown("ArrowRight")) moveDirection = vec3.add(moveDirection, right);
  if (game.clientInputService.isKeyDown("A") || game.clientInputService.isKeyDown("ArrowLeft")) moveDirection = vec3.sub(moveDirection, right);
  const isMoving = vec3.length(moveDirection) > 0;
  const desiredVelocity = isMoving ? vec3.scale(vec3.normalize(moveDirection), settings.speed) : ZERO;
  const desiredFacingAngle = isShiftLocked ? Math.atan2(forward.x, forward.z) : isMoving ? Math.atan2(moveDirection.x, moveDirection.z) : null;
  const isJumping = shouldJump(ground !== null, game.clientInputService.isKeyDown("Space"));

  const autostepDirection = ground === null ? desiredVelocity : vec3.projectOntoPlane(desiredVelocity, vec3.normalize(ground.normal));
  if (vec3.length(autostepDirection) > 0) {
    autostep = {
      startPosition: copyVec3(character.location.position),
      translation: vec3.scale(vec3.normalize(autostepDirection), vec3.length(desiredVelocity) * dtSecs),
      stepHeight: settings.stepHeight,
    };
  }

  const groundVelocity = ground === null ? ZERO : vec3.sub(velocity, game.objectService.getVelocityRelativeToGround(characterObjectId));
  const maxVelocityChange = settings.acceleration * dtSecs;
  let velocityChange: Vec3;
  if (ground !== null && vec3.dot(ground.normal, ground.normal) > 1e-6) {
    const groundNormal = vec3.normalize(ground.normal);
    const inputAlongGround = vec3.projectOntoPlane(desiredVelocity, groundNormal);
    const desiredAlongGround = vec3.add(
      vec3.projectOntoPlane(groundVelocity, groundNormal),
      vec3.dot(inputAlongGround, inputAlongGround) > 1e-6 ? vec3.scale(vec3.normalize(inputAlongGround), vec3.length(desiredVelocity)) : ZERO,
    );
    velocityChange = vec3.clampLength(vec3.sub(desiredAlongGround, vec3.projectOntoPlane(velocity, groundNormal)), maxVelocityChange);
  } else {
    const desiredHorizontal = vec3.add({ x: groundVelocity.x, y: 0, z: groundVelocity.z }, desiredVelocity);
    velocityChange = vec3.clampLength(vec3.sub(desiredHorizontal, { x: velocity.x, y: 0, z: velocity.z }), maxVelocityChange);
  }

  if (isJumping) {
    const gravity = Math.max(-game.objectService.getGravity().y, 0);
    const jumpSpeedChange = Math.max(Math.sqrt(2 * gravity * settings.jumpHeight) - velocity.y, 0);
    velocityChange = vec3.add(velocityChange, { x: 0, y: jumpSpeedChange, z: 0 });
  }

  const axes = game.objectService.getObjectAxes(characterObjectId);
  const flatForward = { x: axes.z.x, y: 0, z: axes.z.z };
  const facingAngle = vec3.dot(flatForward, flatForward) > 1e-6 ? Math.atan2(flatForward.x, flatForward.z) : null;
  let facingError = 0;
  let targetSpinRate = 0;
  if (desiredFacingAngle === null) {
    targetSpinRate = ground === null ? 0 : getGroundSpinRate(ground.object.id);
  } else if (facingAngle !== null) {
    facingError = wrapAngle(desiredFacingAngle - facingAngle);
  }
  const facingAcceleration = clamp(
    facingError * FACING_NATURAL_FREQUENCY * FACING_NATURAL_FREQUENCY
      - (facingSpinRate - targetSpinRate) * 2 * FACING_DAMPING_RATIO * FACING_NATURAL_FREQUENCY,
    -FACING_MAX_ANGULAR_ACCELERATION,
    FACING_MAX_ANGULAR_ACCELERATION,
  );
  facingSpinRate = isShiftLocked ? facingError / SHIFT_LOCK_TURN_SECS : facingSpinRate + facingAcceleration * dtSecs;
  let angularVelocityChange = { x: 0, y: facingSpinRate - angularVelocity.y, z: 0 };

  const tilt = Math.acos(clamp(axes.y.y, -1, 1));
  const balanceSpin = { x: angularVelocity.x, y: 0, z: angularVelocity.z };
  let balanceAxis: Vec3 | null = null;
  if (tilt > 1e-6) {
    const tiltAxis = vec3.cross(axes.y, UP);
    const fallbackTiltAxis = vec3.cross(axes.z, UP);
    if (vec3.dot(tiltAxis, tiltAxis) > 1e-6) balanceAxis = vec3.normalize(tiltAxis);
    else if (vec3.dot(fallbackTiltAxis, fallbackTiltAxis) > 1e-6) balanceAxis = vec3.normalize(fallbackTiltAxis);
    else balanceAxis = { x: 1, y: 0, z: 0 };
  } else if (vec3.dot(balanceSpin, balanceSpin) > 1e-6) {
    balanceAxis = vec3.normalize(balanceSpin);
  }
  if (balanceAxis !== null) {
    const balanceAcceleration = vec3.sub(
      vec3.scale(balanceAxis, tilt * UPRIGHT_NATURAL_FREQUENCY * UPRIGHT_NATURAL_FREQUENCY),
      vec3.scale(balanceSpin, 2 * UPRIGHT_DAMPING_RATIO * UPRIGHT_NATURAL_FREQUENCY),
    );
    angularVelocityChange = vec3.add(angularVelocityChange, vec3.scale(vec3.clampLength(balanceAcceleration, UPRIGHT_MAX_ANGULAR_ACCELERATION), dtSecs));
  }

  const changes: { velocityChange?: Vec3, angularVelocityChange?: Vec3 } = {};
  if (vec3.length(velocityChange) > 0) changes.velocityChange = velocityChange;
  if (vec3.length(angularVelocityChange) > 0) changes.angularVelocityChange = vec3.scale(angularVelocityChange, 1 / RADIANS_PER_DEGREE);
  return changes;
});

game.clientCharacterService.addAfterStepListener((characterObjectId) => {
  const stepAutostep = autostep;
  autostep = null;
  const character = getPhysicalObject(characterObjectId);
  if (character === null) return;
  const position = copyVec3(character.location.position);
  const velocity = copyVec3(character.movement.velocity);

  if (stepAutostep !== null) {
    const direction = vec3.normalize(stepAutostep.translation);
    const distance = vec3.length(stepAutostep.translation);
    const remainingDistance = distance - clamp(vec3.dot(direction, vec3.sub(position, stepAutostep.startPosition)), 0, distance);
    const stepTranslation = remainingDistance > 0 ? getStepTranslation(characterObjectId, vec3.scale(direction, remainingDistance), stepAutostep.stepHeight) : null;
    if (stepTranslation !== null) {
      const steppedVelocity = { x: velocity.x, y: Math.max(velocity.y, 0), z: velocity.z };
      if (steppedVelocity.y <= 0) jumpState = { kind: "groundedReady" };
      else if (jumpState.kind === "waitingToLand" || jumpState.kind === "waitingToGetOffGround") jumpState = { kind: "waitingToGetOffGround", jumpStartMs: nowMs };
      return { position: vec3.add(position, stepTranslation), velocity: steppedVelocity };
    }
  }

  if (jumpState.kind === "waitingToGetOffGround") return;
  const ground = game.objectService.getGroundSupport(characterObjectId, { maxDistance: SNAP_DOWN_HEIGHT, maxGroundSlopeDegrees: MAX_GROUND_SLOPE_DEGREES });
  if (ground === null) return;
  const didLandGently = (jumpState.kind === "offGroundNeverJumped" || jumpState.kind === "waitingToLand") && velocity.y < 0;
  if (jumpState.kind === "waitingToLand" && ground.distance !== 0 && !didLandGently) return;
  const shouldSnap = ground.distance > 0 && (jumpState.kind !== "waitingToLand" || didLandGently);
  jumpState = { kind: "groundedReady" };

  const changes: { position?: Vec3, velocity?: Vec3 } = {};
  if (shouldSnap) changes.position = vec3.sub(position, { x: 0, y: ground.distance, z: 0 });
  if (didLandGently) changes.velocity = { x: velocity.x, y: 0, z: velocity.z };
  return changes;
});
