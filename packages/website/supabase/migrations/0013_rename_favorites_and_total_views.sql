alter table public."games-metadata"
rename column total_plays to total_views;

alter table public.game_bookmarks
rename to game_favorites;

alter table public.game_favorites
rename constraint game_bookmarks_pkey to game_favorites_pkey;

alter table public.game_favorites
rename constraint game_bookmarks_game_id_fkey to game_favorites_game_id_fkey;

alter table public.game_favorites
rename constraint game_bookmarks_profile_id_fkey to game_favorites_profile_id_fkey;

alter index if exists public.game_bookmarks_profile_id_idx
rename to game_favorites_profile_id_idx;
