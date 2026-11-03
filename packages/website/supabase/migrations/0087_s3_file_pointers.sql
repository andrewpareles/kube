-- Files are moving to S3. A column that names the S3 file its row points at ends in _uuid, and a file belongs to one row. The
-- columns that hold the data itself stay until the code stops reading them, and until then a row has the data or the file,
-- never both.
alter table public.game_worlds rename column world_id to world_uuid;
alter table public.game_worlds rename constraint game_worlds_world_id_key to game_worlds_world_uuid_key;

alter table public.game_metadata add column thumbnail_uuid uuid unique;

alter table public.game_editor_worlds add column world_uuid uuid unique;
alter table public.game_editor_worlds alter column gameworld_json drop not null;
alter table public.game_editor_worlds add constraint game_editor_worlds_data_or_file check ((gameworld_json is null) <> (world_uuid is null));

alter table public.asset_metadata add column file_uuid uuid unique;
alter table public.asset_metadata alter column data_json drop not null;
alter table public.asset_metadata add constraint asset_metadata_data_or_file check ((data_json is null) <> (file_uuid is null));

alter table public.catalog_metadata alter column data_json drop not null;

-- Deleting a catalog item takes it from everyone who owns it, without a refund, and off anyone wearing it.
alter table public.user_owned_catalog_items
    drop constraint user_owned_catalog_items_catalog_item_id_fkey,
    add constraint user_owned_catalog_items_catalog_item_id_fkey foreign key (catalog_item_id)
        references public.catalog_metadata (catalog_item_id) on delete cascade;
alter table public.user_profiles
    drop constraint user_profiles_equipped_item_is_owned,
    add constraint user_profiles_equipped_item_is_owned foreign key (id, equipped_catalog_item_id)
        references public.user_owned_catalog_items (profile_id, catalog_item_id) on delete set null (equipped_catalog_item_id);

-- A data row is known by its type and id, so its unused uuid goes and those two become the key.
alter table public.data_metadata
    drop column id,
    drop constraint data_metadata_data_type_data_id_key,
    add primary key (data_type, data_id);
