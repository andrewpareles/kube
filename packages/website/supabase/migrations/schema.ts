import { type AnyPgColumn, pgTable, foreignKey, pgPolicy, pgEnum, uuid, text, timestamp, unique, uniqueIndex, boolean, integer, bigint, primaryKey, check, index, jsonb } from "drizzle-orm/pg-core"
import { sql } from "drizzle-orm"

// 1. No dependencies
export const userProfiles = pgTable("user_profiles", {
	id: uuid().primaryKey().notNull(),
	userId: bigint("user_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "user_profiles_user_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).notNull(),
	email: text(),
	// No default: the name comes from the sign up form, and a name nobody picked could not be unique.
	username: text().notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow(),
	// The item this profile wears. It has to be one they own.
	equippedCatalogItemId: bigint("equipped_catalog_item_id", { mode: "number" }),
	// The color this profile's character wears in games, as #rrggbb.
	preferredColor: text("preferred_color"),
}, (table) => [
	check("user_profiles_username_length", sql`(char_length(${table.username}) between 4 and 16)`),
	check("user_profiles_username_characters", sql`(${table.username} ~ '^[A-Za-z0-9]+$' or ${table.username} = 'Guest-' || ${table.userId})`),
	// case-independent usernames (Andrew and andrew cant appear on 2 different accounts)
	uniqueIndex("user_profiles_username_lower_key").on(sql`lower(${table.username})`),
	// Note: user_profiles.id references auth.users(id) in the database, but we don't model auth.users in Drizzle
	unique("user_profiles_user_id_key").on(table.userId),
	check("user_profiles_preferred_color_is_hex", sql`(${table.preferredColor} ~ '^#[0-9a-f]{6}$')`),
	foreignKey({ columns: [table.id, table.equippedCatalogItemId], foreignColumns: [userOwnedCatalogItems.profileId, userOwnedCatalogItems.catalogItemId], name: "user_profiles_equipped_item_is_owned" }),
	pgPolicy("Users can read own profile", { as: "permissive", for: "select", to: ["public"], using: sql`(auth.uid() = id)` }),
]);

// 2. No dependencies
export const dataTypeEnum = pgEnum("data_type", ["mesh_data", "texture_data", "sound_data"]);

// Editable model sources are separate from the mesh and texture data used at runtime.
export const blenderMetadata = pgTable("blender_metadata", {
	blenderId: bigint("blender_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "blender_metadata_blender_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).primaryKey().notNull(),
	uploadedBy: uuid("uploaded_by").notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	sourceFormat: text("source_format").default('blend').notNull(),
}, (table) => [
	check("blender_metadata_blender_id_check", sql`${table.blenderId} > 0`),
	check("blender_metadata_source_format_check", sql`${table.sourceFormat} in ('blend', 'glb')`),
	foreignKey({ columns: [table.uploadedBy], foreignColumns: [userProfiles.id], name: "blender_metadata_uploaded_by_fkey" }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` }),
]).enableRLS();

// 3. Depends on: userProfiles
export const dataMetadata = pgTable("data_metadata", {
	dataType: dataTypeEnum("data_type").notNull(),
	dataId: bigint("data_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "data_metadata_data_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).notNull(),
	uploadedBy: uuid("uploaded_by").notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	foreignKey({
		columns: [table.uploadedBy],
		foreignColumns: [userProfiles.id],
		name: "data_metadata_uploaded_by_fkey"
	}),
	primaryKey({ columns: [table.dataType, table.dataId], name: "data_metadata_pkey" }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// Asset publications keep their complete payload and a revision within each asset id.
export const assetTypeEnum = pgEnum("asset_type", ["Mesh", "Texture", "Sound", "Object", "Material", "Skybox", "Sfx"]);
export const assetMetadata = pgTable("asset_metadata", {
	assetId: bigint("asset_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "asset_metadata_asset_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).notNull(),
	versionNum: bigint("version_num", { mode: "number" }).notNull(),
	name: text().notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	uploadedBy: uuid("uploaded_by").notNull().references(() => userProfiles.id),
	assetType: assetTypeEnum("asset_type").notNull(),
	assetUuid: uuid("asset_uuid").notNull().unique(),
	importCount: bigint("import_count", { mode: "number" }).default(0).notNull(),
}, (table) => [
	primaryKey({ columns: [table.assetId, table.versionNum], name: "asset_metadata_pkey" }),
	check("asset_metadata_name_check", sql`length(btrim(${table.name})) > 0`),
	check("asset_metadata_import_count_check", sql`${table.importCount} >= 0`),
	index("asset_metadata_uploaded_by_idx").on(table.uploadedBy),
	pgPolicy("Assets are public", { as: "permissive", for: "select", to: ["anon", "authenticated"], using: sql`true` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` }),
]).enableRLS();

// Catalog items keep their own copy of a mesh asset's data, so a later revision of that asset never changes what buyers own.
export const catalogMetadata = pgTable("catalog_metadata", {
	catalogItemId: bigint("catalog_item_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "catalog_metadata_catalog_item_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).primaryKey().notNull(),
	creatorId: uuid("creator_id").notNull().references(() => userProfiles.id, { onDelete: "cascade" }),
	name: text().notNull(),
	description: text(),
	isListed: boolean("is_listed").notNull().default(true),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	updatedAt: timestamp("updated_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	check("catalog_metadata_name_check", sql`length(btrim(${table.name})) between 1 and 100`),
	check("catalog_metadata_description_check", sql`${table.description} is null or length(${table.description}) <= 1000`),
	index("catalog_metadata_creator_id_idx").on(table.creatorId),
	pgPolicy("Anyone can read listed catalog items", { as: "permissive", for: "select", to: ["anon", "authenticated"], using: sql`(is_listed or creator_id = auth.uid())` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` }),
]).enableRLS();

// Deleting an item takes it from everyone who owns it.
export const userOwnedCatalogItems = pgTable("user_owned_catalog_items", {
	profileId: uuid("profile_id").notNull().references((): AnyPgColumn => userProfiles.id, { onDelete: "cascade" }),
	catalogItemId: bigint("catalog_item_id", { mode: "number" }).notNull().references((): AnyPgColumn => catalogMetadata.catalogItemId, { onDelete: "cascade" }),
	purchasedAt: timestamp("purchased_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	primaryKey({ columns: [table.profileId, table.catalogItemId], name: "user_owned_catalog_items_pkey" }),
	index("user_owned_catalog_items_catalog_item_id_idx").on(table.catalogItemId),
	pgPolicy("Users can read what they own", { as: "permissive", for: "select", to: ["authenticated"], using: sql`(profile_id = auth.uid())` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` }),
]).enableRLS();

// 4. No dependencies
export const gameWorlds = pgTable("game_worlds", {
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow(),
	gameId: bigint("game_id", { mode: "number" }).notNull(),
	worldUuid: uuid("world_uuid").notNull().unique(),
	// The game_worlds_set_version_num trigger (migration 0021) overwrites this on insert with the
	// per-game next version; the identity sequence is only a pre-trigger placeholder.
	versionNum: bigint("version_num", { mode: "number" }).generatedByDefaultAsIdentity({ name: "game_worlds_version_num_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).notNull(),
}, (table) => [
	primaryKey({ columns: [table.gameId, table.versionNum], name: "game_worlds_pkey" }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 5. Depends on: userProfiles
export const games = pgTable("game_metadata", {
	gameId: bigint("game_id", { mode: "number" }).generatedByDefaultAsIdentity({ name: "game_metadata_game_id_seq", startWith: 1, increment: 1, minValue: 1, cache: 1 }).primaryKey().notNull(),
	name: text().notNull(),
	description: text(),
	creatorId: uuid("creator_id").notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow(),
	isPublished: boolean("is_published").default(false),
	totalVisits: bigint("total_visits", { mode: "number" }).notNull().default(0),
	suggestedMinPlayers: integer("suggested_min_players").notNull().default(1),
	maxPlayers: integer("max_players").notNull(),
	screenshotUuid: uuid("screenshot_uuid").unique(),
	aiThumbnailUuid: uuid("ai_thumbnail_uuid").unique(),
}, (table) => [
	foreignKey({
		columns: [table.creatorId],
		foreignColumns: [userProfiles.id],
		name: "game_metadata_creator_id_fkey"
	}),
	pgPolicy("Users can read own games", { as: "permissive", for: "select", to: ["public"], using: sql`(auth.uid() = creator_id)` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 6. Depends on: userProfiles, games
export const userLikes = pgTable("user_likes", {
	gameId: bigint("game_id", { mode: "number" }).notNull(),
	profileId: uuid("profile_id").notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	primaryKey({ columns: [table.gameId, table.profileId], name: "user_likes_pkey" }),
	foreignKey({
		columns: [table.gameId],
		foreignColumns: [games.gameId],
		name: "user_likes_game_id_fkey"
	}).onDelete("cascade"),
	foreignKey({
		columns: [table.profileId],
		foreignColumns: [userProfiles.id],
		name: "user_likes_profile_id_fkey"
	}).onDelete("cascade"),
	index("user_likes_profile_id_idx").on(table.profileId),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

export const userFavorites = pgTable("user_favorites", {
	gameId: bigint("game_id", { mode: "number" }).notNull(),
	profileId: uuid("profile_id").notNull(),
	createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, (table) => [
	primaryKey({ columns: [table.gameId, table.profileId], name: "user_favorites_pkey" }),
	foreignKey({
		columns: [table.gameId],
		foreignColumns: [games.gameId],
		name: "user_favorites_game_id_fkey"
	}).onDelete("cascade"),
	foreignKey({
		columns: [table.profileId],
		foreignColumns: [userProfiles.id],
		name: "user_favorites_profile_id_fkey"
	}).onDelete("cascade"),
	index("user_favorites_profile_id_idx").on(table.profileId),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 7. Depends on: games, gameWorlds
export const startingServers = pgTable("starting_servers", {
	requestId: text("request_id").primaryKey().notNull(),
	gameId: bigint("game_id", { mode: "number" }).notNull(),
	gameworldVersionNum: bigint("gameworld_version_num", { mode: "number" }).notNull(),
	maxPlayers: integer("max_players").notNull(),
	startingAt: timestamp("starting_at", { withTimezone: true, mode: 'string' }).notNull(),
}, (table) => [
	foreignKey({
		columns: [table.gameId],
		foreignColumns: [games.gameId],
		name: "starting_servers_game_id_fkey"
	}),
	foreignKey({
		columns: [table.gameId, table.gameworldVersionNum],
		foreignColumns: [gameWorlds.gameId, gameWorlds.versionNum],
		name: "starting_servers_game_world_fkey"
	}),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 8. Depends on: games, gameWorlds
// Chat from every running server, written after the fact so it outlives the server that held it.
export const gameChats = pgTable("game_chats", {
    gameChatId: bigint("game_chat_id", { mode: "number" }).generatedByDefaultAsIdentity().primaryKey(),
    serverid: text("serverid").notNull(),
    timeLiveServer: timestamp("time_live_server", { withTimezone: true, mode: 'string' }).notNull(),
    userid: text("userid").notNull(),
    message: text("message").notNull(),
}, table => [
    index("game_chats_serverid_time_idx").on(table.serverid, table.timeLiveServer),
    index("game_chats_userid_time_idx").on(table.userid, table.timeLiveServer),
    pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` }),
]).enableRLS();

export const runningServers = pgTable("running_servers", {
	requestId: text("request_id").primaryKey().notNull(),
	gameId: bigint("game_id", { mode: "number" }).notNull(),
	gameworldVersionNum: bigint("gameworld_version_num", { mode: "number" }).notNull(),
	maxPlayers: integer("max_players").notNull(),
	publicIp: text("public_ip").notNull(),
	externalPort: integer("external_port").notNull(),
}, (table) => [
	foreignKey({
		columns: [table.gameId],
		foreignColumns: [games.gameId],
		name: "running_servers_game_id_fkey"
	}),
	foreignKey({
		columns: [table.gameId, table.gameworldVersionNum],
		foreignColumns: [gameWorlds.gameId, gameWorlds.versionNum],
		name: "running_servers_game_world_fkey"
	}),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 9. Depends on: userProfiles
export const userDesktopPlayAuth = pgTable("user_desktop_play_auth", {
	profileId: uuid("profile_id").primaryKey().notNull(),
	playAuthHash: text("play_auth_hash"),
}, (table) => [
	foreignKey({
		columns: [table.profileId],
		foreignColumns: [userProfiles.id],
		name: "player_auth_user_profile_id_fkey"
	}),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 10. Depends on: userProfiles, startingServers
export const startingPlayerSessions = pgTable("starting_player_sessions", {
	profileId: uuid("profile_id").primaryKey().notNull(),
	requestId: text("request_id").notNull(),
}, (table) => [
	foreignKey({
		columns: [table.profileId],
		foreignColumns: [userProfiles.id],
		name: "starting_player_sessions_user_profile_id_fkey"
	}),
	foreignKey({
		columns: [table.requestId],
		foreignColumns: [startingServers.requestId],
		name: "starting_player_sessions_request_id_fkey"
	}).onDelete("cascade"),
	pgPolicy("Users can read own sessions", { as: "permissive", for: "select", to: ["public"], using: sql`(auth.uid() = profile_id)` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// 11. Depends on: userProfiles, runningServers
export const runningPlayerSessions = pgTable("running_player_sessions", {
	profileId: uuid("profile_id").primaryKey().notNull(),
	requestId: text("request_id").notNull(),
	// The row itself is the player's ticket into the server and is written when they press Play.
	// connected_at is stamped by the game server when they actually connect, so only a stamped row
	// means someone is really in the game. See 0030_running_player_session_connected_at.sql.
	reservedAt: timestamp("reserved_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
	connectedAt: timestamp("connected_at", { withTimezone: true, mode: 'string' }),
}, (table) => [
	foreignKey({
		columns: [table.profileId],
		foreignColumns: [userProfiles.id],
		name: "running_player_sessions_user_profile_id_fkey"
	}),
	foreignKey({
		columns: [table.requestId],
		foreignColumns: [runningServers.requestId],
		name: "running_player_sessions_request_id_fkey"
	}).onDelete("cascade"),
	pgPolicy("Users can read own sessions", { as: "permissive", for: "select", to: ["public"], using: sql`(auth.uid() = profile_id)` }),
	pgPolicy("Service role only", { as: "permissive", for: "all", to: ["service_role"] }),
]);

// Browser editors keep unpublished worlds separate from public game versions and hosted runtimes.
export const editorShareAuth = pgTable("editor_share_auth", {
    gameId: bigint("game_id", { mode: "number" }).primaryKey().references(() => games.gameId, { onDelete: "cascade" }),
    inviteToken: text("invite_token"),
}, () => [pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();
export const gameEditorWorlds = pgTable("game_editor_worlds", {
    gameId: bigint("game_id", { mode: "number" }).primaryKey().references(() => games.gameId, { onDelete: "cascade" }),
    worldUuid: uuid("world_uuid").notNull().unique(),
    updatedAt: timestamp("updated_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, table => [pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();
export const startingEditorServers = pgTable("starting_editor_servers", {
    requestId: text("request_id").primaryKey(),
    gameId: bigint("game_id", { mode: "number" }).notNull().unique().references(() => games.gameId, { onDelete: "cascade" }),
    startingAt: timestamp("starting_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, () => [pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();
export const runningEditorServers = pgTable("running_editor_servers", {
    requestId: text("request_id").primaryKey(),
    gameId: bigint("game_id", { mode: "number" }).notNull().unique().references(() => games.gameId, { onDelete: "cascade" }),
    editorUrl: text("editor_url").notNull(),
    publicIp: text("public_ip").notNull(),
    playtestExternalPort: integer("playtest_external_port").notNull(),
}, table => [check("running_editor_servers_playtest_external_port_check", sql`${table.playtestExternalPort} between 1 and 65535`), pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();

// Each message is an AI SDK UIMessage. The usage ledger has one row per model call.
export const chatMessages = pgTable("chat_messages", {
    gameId: bigint("game_id", { mode: "number" }).notNull().references(() => games.gameId, { onDelete: "cascade" }),
    seq: bigint("seq", { mode: "number" }).notNull(),
    message: jsonb("message").notNull(),
    createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, table => [primaryKey({ columns: [table.gameId, table.seq] }), pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();
export const chatUsage = pgTable("chat_usage", {
    id: bigint("id", { mode: "number" }).generatedAlwaysAsIdentity().primaryKey(),
    profileId: uuid("profile_id").notNull().references(() => userProfiles.id, { onDelete: "cascade" }),
    gameId: bigint("game_id", { mode: "number" }).references(() => games.gameId, { onDelete: "set null" }),
    modelId: text("model_id").notNull(),
    inputTokens: integer("input_tokens").notNull().default(0),
    cacheWriteTokens: integer("cache_write_tokens").notNull().default(0),
    cacheReadTokens: integer("cache_read_tokens").notNull().default(0),
    outputTokens: integer("output_tokens"),
    awsRequestId: text("aws_request_id").notNull(),
    // What the account was charged for this call. The token counts beside it stay raw so they survive
    // a change to the rate or the margin; this one never moves again.
    creditsCharged: integer("credits_charged").notNull(),
    // The chat reply this call was made for. Null for calls made before it was recorded.
    messageId: text("message_id"),
    createdAt: timestamp("created_at", { withTimezone: true, mode: 'string' }).defaultNow().notNull(),
}, table => [index("chat_usage_profile_created_idx").on(table.profileId, table.createdAt), uniqueIndex("chat_usage_aws_request_id_key").on(table.awsRequestId), pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();

export const planIdEnum = pgEnum("plan_id", ["pro", "max"]);

// A copy of what Stripe says an account is paying for. Kept after cancelling. No row, or a status
// that is not active, means the account is on the free tier.
export const userSubscriptions = pgTable("user_subscriptions", {
    profileId: uuid("profile_id").primaryKey().references(() => userProfiles.id, { onDelete: "cascade" }),
    plan: planIdEnum("plan").notNull(),
    // Text and not an enum, so a status Stripe adds later cannot throw on the webhook carrying it.
    status: text("status").notNull(),
    stripeCustomerId: text("stripe_customer_id").notNull().unique(),
    currentPeriodEnd: timestamp("current_period_end", { withTimezone: true, mode: 'string' }).notNull(),
}, table => [pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();

// Site admins see every game on /admin and open any game's editor the way someone the creator invited would.
export const siteAdmins = pgTable("site_admins", {
    profileId: uuid("profile_id").primaryKey().references(() => userProfiles.id, { onDelete: "cascade" }),
}, () => [pgPolicy("Service role only", { for: "all", to: ["service_role"], using: sql`true`, withCheck: sql`true` })]).enableRLS();
