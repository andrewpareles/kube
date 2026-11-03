import { relations } from "drizzle-orm/relations";
import { userProfiles, userDesktopPlayAuth, startingPlayerSessions, runningPlayerSessions, startingServers, runningServers, games, gameWorlds, userLikes, userFavorites, assetMetadata, blenderMetadata } from "./schema";

export const blenderMetadataRelations = relations(blenderMetadata, ({one}) => ({
	profile: one(userProfiles, {
		fields: [blenderMetadata.uploadedBy],
		references: [userProfiles.id]
	}),
}));

export const userDesktopPlayAuthRelations = relations(userDesktopPlayAuth, ({one}) => ({
	profile: one(userProfiles, {
		fields: [userDesktopPlayAuth.profileId],
		references: [userProfiles.id]
	}),
}));

export const startingPlayerSessionsRelations = relations(startingPlayerSessions, ({one}) => ({
	profile: one(userProfiles, {
		fields: [startingPlayerSessions.profileId],
		references: [userProfiles.id]
	}),
	startingServer: one(startingServers, {
		fields: [startingPlayerSessions.requestId],
		references: [startingServers.requestId]
	}),
}));

export const runningPlayerSessionsRelations = relations(runningPlayerSessions, ({one}) => ({
	profile: one(userProfiles, {
		fields: [runningPlayerSessions.profileId],
		references: [userProfiles.id]
	}),
	runningServer: one(runningServers, {
		fields: [runningPlayerSessions.requestId],
		references: [runningServers.requestId]
	}),
}));

export const userProfilesRelations = relations(userProfiles, ({many, one}) => ({
	userDesktopPlayAuth: one(userDesktopPlayAuth, {
		fields: [userProfiles.id],
		references: [userDesktopPlayAuth.profileId]
	}),
	startingPlayerSession: one(startingPlayerSessions, {
		fields: [userProfiles.id],
		references: [startingPlayerSessions.profileId]
	}),
	runningPlayerSession: one(runningPlayerSessions, {
		fields: [userProfiles.id],
		references: [runningPlayerSessions.profileId]
	}),
	userLikes: many(userLikes),
	userFavorites: many(userFavorites),
	games: many(games),
	assets: many(assetMetadata),
	blenderFiles: many(blenderMetadata),
}));

export const startingServersRelations = relations(startingServers, ({one, many}) => ({
	startingPlayerSessions: many(startingPlayerSessions),
	game: one(games, {
		fields: [startingServers.gameId],
		references: [games.gameId]
	}),
	gameworld: one(gameWorlds, {
		fields: [startingServers.gameId, startingServers.gameworldVersionNum],
		references: [gameWorlds.gameId, gameWorlds.versionNum]
	}),
}));

export const runningServersRelations = relations(runningServers, ({one, many}) => ({
	runningPlayerSessions: many(runningPlayerSessions),
	game: one(games, {
		fields: [runningServers.gameId],
		references: [games.gameId]
	}),
	gameworld: one(gameWorlds, {
		fields: [runningServers.gameId, runningServers.gameworldVersionNum],
		references: [gameWorlds.gameId, gameWorlds.versionNum]
	}),
}));

export const gamesRelations = relations(games, ({one, many}) => ({
	profile: one(userProfiles, {
		fields: [games.creatorId],
		references: [userProfiles.id]
	}),
	startingServers: many(startingServers),
	runningServers: many(runningServers),
	userLikes: many(userLikes),
	userFavorites: many(userFavorites),
}));

export const userLikesRelations = relations(userLikes, ({one}) => ({
	game: one(games, {
		fields: [userLikes.gameId],
		references: [games.gameId]
	}),
	profile: one(userProfiles, {
		fields: [userLikes.profileId],
		references: [userProfiles.id]
	}),
}));

export const userFavoritesRelations = relations(userFavorites, ({one}) => ({
	game: one(games, {
		fields: [userFavorites.gameId],
		references: [games.gameId]
	}),
	profile: one(userProfiles, {
		fields: [userFavorites.profileId],
		references: [userProfiles.id]
	}),
}));

export const gameWorldsRelations = relations(gameWorlds, ({many}) => ({
	games: many(games),
	startingServers: many(startingServers),
	runningServers: many(runningServers),
}));

export const assetMetadataRelations = relations(assetMetadata, ({one}) => ({
	publisher: one(userProfiles, {
		fields: [assetMetadata.uploadedBy],
		references: [userProfiles.id]
	}),
}));
