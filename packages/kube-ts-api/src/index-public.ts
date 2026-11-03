// These are the types we expose to AI chat and public docs. Bundled with api-extractor.
// They don't include internal __ops__ that are only used internally.

export { createState, game, getDelayedWorldTime, getTime, script, stopScript, tick, useState, vec3 } from "./index"

export type { LiveObject } from "./world"

export type * from "./object_types/shared"
export type * from "./object_types/types"

export type * from "./playerService"
export type { ObjectCreatedCallback, ObjectDeletedCallback, RaycastHit, RaycastOptions, ShapeCastOptions, GroundSupportOptions, ObjectService } from "./objectService"
export type * from "./templateService"
export type * from "./animationService"
export type * from "./clockService"
export type * from "./collisionService"
export type * from "./gameChatService"
export type * from "./clientInputService"
export type * from "./clientCharacterService"
export type * from "./vec3"
export type { GuiRenderable, GuiRootRenderable } from "./gui/guiTypes"
export type { GuiService } from "./gui/guiService"
