import superjson from "superjson"
import { getRustObject, type LiveObject, world } from "./world"
import { playerService } from "./playerService"
import { objectService } from "./objectService"
import { templateService } from "./templateService"
import { animationService } from "./animationService"
import { clockService } from "./clockService"
import { collisionService } from "./collisionService"
import { gameChatService } from "./gameChatService"
import { clientInputService } from "./clientInputService"
import { clientCharacterService } from "./clientCharacterService"
import type { ObjectId } from "./object_types/shared"
import { denoOps } from "./object_types/__globalOps__"
import { createScriptGuiService, createState, useState } from "./gui/guiService"
import "./jsCallbacks" // important to mount this


// ------------------------------- helpers -------------------------------

// this reads the script object id that Rust injects into the script
const parseScriptObjectId = (__script_object_id__: unknown): ObjectId => {
  const scriptObjectId = typeof __script_object_id__ === "number" ? __script_object_id__ : null
  if (scriptObjectId === null) throw new Error("Script id is not available")
  return scriptObjectId
}

// ------------------------------- internal use only -------------------------------

// __script_object_id__ is automatically passed in by us when the game runs
export const __getGame__ = (scriptObjectId: ObjectId | undefined) => {
  return {
    world,
    playerService,
    objectService,
    templateService,
    animationService,
    clockService,
    collisionService,
    clientInputService,
    clientCharacterService,
    gameChatService,
    guiService: createScriptGuiService(scriptObjectId),
  }
}
export const __getScript__ = (__script_object_id__: ObjectId | undefined): LiveObject => {
  if (__script_object_id__ === undefined) return null as never
  const scriptObjectId = parseScriptObjectId(__script_object_id__)
  const scriptObject = getRustObject(scriptObjectId)
  if (scriptObject === null) throw new Error("Unexpected error: Script object was not found when reading `script` variable.")
  return scriptObject
}


// ------------------------------- public -------------------------------

// automatically re-defined by us to take in the script object id instead of undefined
export const game = __getGame__(undefined)
export const script = __getScript__(undefined)

// __script_object_id__ is automatically passed in by us when the game runs
/** Use `await tick()` to await the next tick of the game loop. The game ticks 60 times a second on both the server and clients. Always use this in places like infinite while loops, otherwise the script will stall and be killed. */
export const tick = async (__script_object_id__?: unknown): Promise<void> => {
  const scriptObjectId = parseScriptObjectId(__script_object_id__)
  await denoOps.__tickInternal__(scriptObjectId)
}

/** Returns the elapsed time of the game's server in milliseconds.
 * On the server this is the time of the current tick. On a client this is an estimate that is continuously synced to the server's clock, so every player's `getTime()` roughly agrees.
 * The value is fixed at the start of the current tick or callback run, so repeated calls within one run return the same time.
 * Useful for shared timers: for example, the server sends a match end time to clients, and each client renders `endTime - getTime()`. */
export const getTime = (): number => {
  return denoOps.__getTimeMs__()
}

/** Returns `getTime()` minus this client's world delay (about 100 ms plus its ping), so it is always earlier than `getTime()`.
 * In a client's world, other players and everything the server moves are this far in the past so they move smoothly. Your own character is at `getTime()`.
 * Pass it to a `'use server'` function to say when you saw something, e.g. for `game.playerService.getCharacterLocationAtTime`.
 * Like `getTime()`, it is fixed at the start of the current tick or callback run.
 * Only callable by a Client Script. */
export const getDelayedWorldTime = (): number => {
  return denoOps.__getDelayedWorldTimeMs__()
}

/** Use `await stopScript()` to stop all further execution of a script. */
export const stopScript = async (): Promise<never> => {
  return await denoOps.__stopScriptInternal__()
}

// internal server/client-function stuff
// Remote-call serialization stays in JavaScript; Rust transports only encoded strings.
const wrapRemoteFn = (fn: (...args: unknown[]) => unknown) => async (args: string): Promise<string> => {
  if (!denoOps.__isFnFromLiveScript__(fn)) throw new Error("This function's script was deleted.")
  return superjson.stringify(await fn(...superjson.parse<unknown[]>(args)))
}

export const __registerJsServerFn__ = (fnIdentifier: string, serverFn: (...args: unknown[]) => unknown) => {
  denoOps.__registerJsServerFn__(fnIdentifier, wrapRemoteFn(serverFn))
}

export const __callJsClientFnFromServer__ = async (playerObjectId: ObjectId, fnIdentifier: string, ...args: unknown[]) => {
  // playerObjectId is both the routing key and the client function's first parameter.
  const jsResult = await denoOps.__callJsClientFnFromServer__(playerObjectId, fnIdentifier, superjson.stringify([playerObjectId, ...args]))
  return superjson.parse(jsResult)
}

export const __registerJsClientFn__ = (fnIdentifier: string, clientFn: (...args: unknown[]) => unknown) => {
  denoOps.__registerJsClientFn__(fnIdentifier, wrapRemoteFn(clientFn))
}

export const __callJsServerFnFromClient__ = async (fnIdentifier: string, ...args: unknown[]) => {
  const jsResult = await denoOps.__callJsServerFnFromClient__(fnIdentifier, superjson.stringify(args))
  return superjson.parse(jsResult)
}

export { createState, useState }
export { vec3 } from "./vec3"

// ------------------------------- public types -------------------------------

// export all object types
export type * from "./object_types/types"

// export all service types - very important so the doc strings survive for the ai chat (the api.d.ts file from microsoft api builder), and we probably want to expose this to the user anyway
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
export type { GuiService } from "./gui/guiService"
export type { GuiRenderable, GuiRootRenderable } from "./gui/guiTypes"
