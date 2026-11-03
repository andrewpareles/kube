'use client'
// Fades out the local player's own character as their camera zooms in, so it doesn't block their view. Only this player sees it fade.
// The character is the RigidGroup that holds the camera's head, or just the head when it isn't in one.

const FADE_START_ZOOM = 3; // TODO: AI-PICKED-VALUE: the character starts fading when the camera is this close to the head. The default character is about 4 tall with a 1 wide head.
const FADE_END_ZOOM = 1; // TODO: AI-PICKED-VALUE: the character is fully hidden when the camera is this close to the head, about when the head fills the view.

type LiveObject = NonNullable<ReturnType<typeof game.objectService.getObjectById>>;
type Part = Extract<LiveObject, { opacity: number }>;

// Each faded part's own opacity, and the opacity this script gave it. A part whose opacity isn't the one this script gave it was changed
// by the game, so that is its own opacity now.
const opacitiesOfPartId: { [partId: number]: { ownOpacity: number; fadedOpacity: number } } = {};

const getCharacter = (head: LiveObject): LiveObject => {
  for (let object: LiveObject | null = head; object !== null; object = object.parent) {
    if (object.type === "rigidGroup") return object;
  }
  return head;
};

const getParts = (object: LiveObject): Part[] => [...("opacity" in object ? [object] : []), ...object.getChildren().flatMap(getParts)];

while (true) {
  const player = game.objectService.getObjectById(game.playerService.getLocalPlayerObjectId());
  const head = player?.type === "player" && player.headObjectId !== null ? game.objectService.getObjectById(player.headObjectId) : null;
  const visibility = Math.min(Math.max((game.clientInputService.getZoom() - FADE_END_ZOOM) / (FADE_START_ZOOM - FADE_END_ZOOM), 0), 1);
  const parts = head === null || visibility === 1 ? [] : getParts(getCharacter(head));

  // Parts that stop fading, like when the camera zooms back out or the player gets a new character, get their own opacity back.
  const partIds = new Set(parts.map((part) => part.id));
  for (const partId of Object.keys(opacitiesOfPartId).map(Number)) {
    if (partIds.has(partId)) continue;
    const { ownOpacity, fadedOpacity } = opacitiesOfPartId[partId];
    delete opacitiesOfPartId[partId];
    const part = game.objectService.getObjectById(partId);
    if (part !== null && "opacity" in part && part.opacity === fadedOpacity && part.opacity !== ownOpacity) part.opacity = ownOpacity;
  }

  // Faded opacities are rounded to 3 decimals, so they read back exactly as they were written.
  for (const part of parts) {
    const opacities = opacitiesOfPartId[part.id];
    const ownOpacity = opacities === undefined || part.opacity !== opacities.fadedOpacity ? part.opacity : opacities.ownOpacity;
    const fadedOpacity = Math.round(ownOpacity * visibility * 1000) / 1000;
    if (part.opacity !== fadedOpacity) part.opacity = fadedOpacity;
    opacitiesOfPartId[part.id] = { ownOpacity, fadedOpacity };
  }

  await tick();
}
