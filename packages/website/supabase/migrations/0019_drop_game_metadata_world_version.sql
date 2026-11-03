alter table public.game_metadata
drop constraint if exists game_metadata_game_world_fkey;

alter table public.game_metadata
drop column if exists world_version;
