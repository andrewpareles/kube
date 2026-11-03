-- Drop the unused chat_messages.world_version hook. It was meant to link a chat turn to a
-- game_worlds checkpoint, but game_worlds holds *published* world versions only, so per-turn world
-- backups must not reference it. The column was never populated (no code path wrote it). When
-- per-turn world backups are built they will use their own independent store, not game_worlds.

alter table public.chat_messages
drop column if exists world_version;
