import { denoOps } from "./object_types/__globalOps__";
import { objectService } from "./objectService";

import type { Location3D, ObjectId, Vec3 } from "./object_types/shared";
import type { LiveObject } from "./world";

export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

/**
 *
 * To deal with players joining or leaving, use the object service.
 * - Join: `game.objectService.addObjectCreatedListener((player) => { ... }, "player")`
 * - Leave: `game.objectService.addObjectDeletedListener((playerObjectId) => { ... }, "player")`.
 *
 * */
export type PlayerService = {
    getAllPlayers: () => (LiveObject & { type: "player" })[],
    /** Only callable by a Client Script. */
    getLocalPlayerObjectId: () => ObjectId,
    /** 
     * Share state about your player with the server and other clients. For example, which animation is currently playing on your character.
     * 
     * Note: It is sent urgently, so you can use it for e.g. animations. Numbers are rounded to 3 decimal places to save bandwidth.
     *
     * Throws if the metadata is over 256 bytes as JSON.
     *
     * Only callable by a Client Script.
     *  */
    setLocalPlayerMetadata: (metadata: JsonValue) => void,
    /** The last JSON value received from this player, or null. */
    getPlayerMetadata: (playerObjectId: ObjectId) => JsonValue | null,
    /**  */
    getPlayerCameraDirection: (playerObjectId: ObjectId) => Vec3 | null,
    /** The ping (round trip) measured from the server. On clients it updates once a second. */
    getPlayerPing: (playerObjectId: ObjectId) => number | null,
    /** Fetches this player's kube.new profile again and updates their `preferredColor` and `equippedItemId`. Only callable by a Server Script. */
    refreshPlayerProfile: (playerObjectId: ObjectId) => Promise<void>,
    /**
     * Use this in a `'use server'` function to see which player called it. Null if called by the server.
     *
     * Only callable by a Server Script.
     */
    getServerFnCaller: () => ObjectId | null,
    /**
     * Returns the location of the character object at the given time. `timeMs` is in units of the `getTime()` clock. It must be recent (within the last second).
     * Returns null for old times and non-characters.
     * Only callable by a Server Script.
     */
    getCharacterLocationAtTime: (objectId: ObjectId, timeMs: number) => Location3D | null,
    /**
    * Returns the metadata of the player at the given time. `timeMs` is on the `getTime()` clock and must be recent (within the last second).
     * Returns null for old times, and times when metadata was null.
     * Only callable by a Server Script.
     */
    getPlayerMetadataAtTime: (playerObjectId: ObjectId, timeMs: number) => JsonValue | null,
}



export const playerService: PlayerService = {
    getLocalPlayerObjectId: () => denoOps.__getRustLocalPlayerObjectId__(),
    setLocalPlayerMetadata: (metadata: JsonValue) => {
        // we round all numbers here so they dont take up too much space like 1.00005932.
        const metadataJson = JSON.stringify(metadata, (_key, value) => typeof value === "number" ? Math.round(value * 1000) / 1000 : value);
        if (metadataJson === undefined) throw new Error("setLocalPlayerMetadata: the metadata must be a JSON value");
        denoOps.__setLocalPlayerMetadata__(metadataJson);
    },
    getPlayerMetadata: (playerObjectId: ObjectId) => {
        const metadataJson = denoOps.__getPlayerMetadata__(playerObjectId);
        return metadataJson === null ? null : JSON.parse(metadataJson);
    },
    // TODO: scripts can only read a player's camera. Writing or animating it (eg with a PropertyAnimator) makes us want to create a new camera object type.
    getPlayerCameraDirection: (playerObjectId: ObjectId) => denoOps.__getPlayerCameraDirection__(playerObjectId),
    getPlayerPing: (playerObjectId: ObjectId) => denoOps.__getPlayerPing__(playerObjectId),
    refreshPlayerProfile: async (playerObjectId: ObjectId) => {
        const { preferredColor, equippedItemId } = await denoOps.__fetchPlayerProfile__(playerObjectId);
        const player = objectService.getObjectById(playerObjectId);
        if (player === null || player.type !== "player") return;
        player.preferredColor = preferredColor;
        player.equippedItemId = equippedItemId;
    },
    getServerFnCaller: () => denoOps.__getServerFnCaller__(),
    getCharacterLocationAtTime: (objectId: ObjectId, timeMs: number) => denoOps.__getCharacterLocationAtTime__(objectId, timeMs),
    getPlayerMetadataAtTime: (playerObjectId: ObjectId, timeMs: number) => {
        const metadataJson = denoOps.__getPlayerMetadataAtTime__(playerObjectId, timeMs);
        return metadataJson === null ? null : JSON.parse(metadataJson);
    },
    getAllPlayers: () => {
        const players: (LiveObject & { type: "player" })[] = [];
        for (const player of objectService.getObjectsByType("player")) {
            if (player.type !== "player") continue; // should never happen
            players.push(player);
        }
        return players;
    },
}
