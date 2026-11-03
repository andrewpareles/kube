-- The suggested least number of players a game needs to start, shown beside max players. Nothing enforces it.
alter table public.game_metadata add column suggested_min_players integer not null default 1;
