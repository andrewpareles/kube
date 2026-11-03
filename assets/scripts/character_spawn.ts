'use server'
// Gives each player their own copy of the Character in CharacterController (or next to this script), standing where the
// Character stands, and brings players who fall off the world back there. Edit the Character to change how players look, or
// this script to change where and how they spawn.

const RESPAWN_BELOW_Y = -50;
const RESPAWN_FALL_SPEED = 20;
const RESPAWN_CHECK_MS = 250;

const template = game.objectService.getObjectByName("CharacterController")?.getChild("Character") ?? script.parent?.getChild("Character") ?? null;
const templateBody = template?.getChild("Body") ?? null;
if (template === null || templateBody === null || !("location" in templateBody)) {
  throw new Error("The spawn script needs a Character with a Body, in CharacterController or next to the spawn script.");
}
const { x, y, z } = templateBody.location.position;
const spawnPosition = { x, y, z };
game.templateService.cutObjectToTemplate(template.id, "Character");

const characterIdOfPlayerId = new Map<number, number>();

game.objectService.addObjectCreatedListener((player) => {
  if (player.type !== "player") return;
  const character = game.templateService.pasteTemplate("Character", game.world.id);
  characterIdOfPlayerId.set(player.id, character.id);
  const body = character.getChild("Body");
  if (body === null) throw new Error("The Character needs a Body.");
  const nametag = character.getChild("Nametag");
  if (nametag?.type === "label") nametag.text = player.username;
  // A Character that isn't pushable would leave every player stuck in place.
  game.objectService.setObjectPushable(body.id, true);
  player.characterObjectId = body.id;
  player.headObjectId = (body.getChild("Head") ?? body).id;
}, "player");

game.objectService.addObjectDeletedListener((playerId) => {
  const characterId = characterIdOfPlayerId.get(playerId);
  if (characterId === undefined) return;
  characterIdOfPlayerId.delete(playerId);
  game.objectService.deleteObject(characterId);
}, "player");

setInterval(() => {
  for (const player of game.playerService.getAllPlayers()) {
    if (player.characterObjectId === null) continue;
    const body = game.objectService.getObjectById(player.characterObjectId);
    if (body === null || !("movement" in body)) continue;
    if (body.location.position.y > RESPAWN_BELOW_Y || body.movement.velocity.y > -RESPAWN_FALL_SPEED) continue;
    game.objectService.teleportObjectTo(body.id, spawnPosition);
  }
}, RESPAWN_CHECK_MS);
