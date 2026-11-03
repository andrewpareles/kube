insert into storage.buckets (id, name, public)
values ('game-worlds', 'game-worlds', false)
on conflict (id) do update
set name = excluded.name,
    public = excluded.public;

alter table public.game_worlds
    add column world_id uuid unique,
    alter column gameworld_json drop not null;
