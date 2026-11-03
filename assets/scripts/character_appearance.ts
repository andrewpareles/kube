'use server'
// Dresses each player's character from their kube.new profile: their preferred color paints the Body's parts named below, and their
// hat goes on the Body's HatSpot in place of its Cap. Hats are made for a 1x1x1 head, so each one is scaled to the width of the Head.
// A game can swap a player's character at any time, like on respawn, so every player's character is checked every tick.
// A game can also swap parts inside a character, and a player can change their profile, so every few seconds each player's profile is
// fetched again and their character is dressed again.

// The parts of the Body that each player's preferred color paints, and the color guests get.
const PREFERRED_COLOR_PARTS = ["Torso", "Cap"];
const GUEST_COLOR = { r: 0.25, g: 0.55, b: 0.9 };
const REDRESS_MS = 10_000;

const dressedCharacterIdOfPlayerId = new Map<number, number>();
// The hat on each HatSpot and the catalog item it came from, so dressing again only changes it when the player equips a different one.
const hatOfHatSpotId = new Map<number, { itemId: number; hatId: number }>();
const hatSpotIdsGettingHat = new Set<number>();

game.objectService.addObjectDeletedListener((playerId) => {
  dressedCharacterIdOfPlayerId.delete(playerId);
}, "player");

type Player = ReturnType<typeof game.playerService.getAllPlayers>[number];
const dress = async (body: NonNullable<ReturnType<typeof game.objectService.getObjectById>>, { preferredColor, equippedItemId }: Player) => {
  for (const partName of PREFERRED_COLOR_PARTS) {
    const part = body.getChild(partName);
    if (part !== null && "color" in part) part.color = preferredColor ?? GUEST_COLOR;
  }
  const hatSpot = body.getChild("HatSpot");
  if (hatSpot === null || hatSpotIdsGettingHat.has(hatSpot.id)) return;
  const wornHat = hatOfHatSpotId.get(hatSpot.id);
  if (wornHat !== undefined && wornHat.itemId !== equippedItemId) {
    game.objectService.deleteObject(wornHat.hatId);
    hatOfHatSpotId.delete(hatSpot.id);
  }
  if (equippedItemId === null) return;
  if (!hatOfHatSpotId.has(hatSpot.id)) {
    hatSpotIdsGettingHat.add(hatSpot.id);
    const hat = await game.objectService.addCatalogItemToWorld(equippedItemId, hatSpot.id);
    hatSpotIdsGettingHat.delete(hatSpot.id);
    if (hat === null) return;
    hatOfHatSpotId.set(hatSpot.id, { itemId: equippedItemId, hatId: hat.id });
    const head = body.getChild("Head");
    const headWidth = head === null ? 1 : "size" in head ? head.size.x : "radius" in head ? head.radius * 2 : 1;
    // The hat's Meshes follow the HatSpot so they move with the head. Its own physics_collider would be a separate body that falls off.
    for (const part of hat.getChildren()) {
      if (part.type === "mesh") {
        part.physicalObjectId = hatSpot.id;
        part.scale = vec3.scale(part.scale, headWidth);
      }
      if (part.name === "physics_collider") game.objectService.deleteObject(part.id);
    }
  }
  const cap = body.getChild("Cap");
  if (cap !== null) game.objectService.deleteObject(cap.id);
};

setInterval(() => {
  for (const player of game.playerService.getAllPlayers()) {
    game.playerService.refreshPlayerProfile(player.id).then(() => {
      // The player may have left while their profile was fetched.
      if (game.objectService.getObjectById(player.id) === null || player.characterObjectId === null) return;
      const body = game.objectService.getObjectById(player.characterObjectId);
      if (body !== null) dress(body, player);
    });
  }
}, REDRESS_MS);

while (true) {
  for (const player of game.playerService.getAllPlayers()) {
    const characterId = player.characterObjectId;
    if (characterId === null || dressedCharacterIdOfPlayerId.get(player.id) === characterId) continue;
    const body = game.objectService.getObjectById(characterId);
    if (body === null) continue;
    dressedCharacterIdOfPlayerId.set(player.id, characterId);
    dress(body, player);
  }
  await tick();
}
