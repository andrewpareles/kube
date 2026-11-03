alter table public.game_worlds
    alter column world_id set not null,
    drop column gameworld_json;
