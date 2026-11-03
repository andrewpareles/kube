-- Every file is in S3 now, so the columns that held the data itself are dropped, and every row points at its file.
alter table public.game_editor_worlds
    drop constraint game_editor_worlds_data_or_file,
    drop column gameworld_json,
    alter column world_uuid set not null;

alter table public.asset_metadata
    drop constraint asset_metadata_data_or_file,
    drop constraint asset_metadata_data_type_check,
    drop column data_json,
    alter column asset_uuid set not null;

alter table public.catalog_metadata
    drop constraint catalog_metadata_data_type_check,
    drop column data_json;
