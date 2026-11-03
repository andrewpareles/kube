-- A column that names an S3 file is named after what the file is, like thumbnail_uuid and world_uuid.
alter table public.asset_metadata rename column file_uuid to asset_uuid;
alter table public.asset_metadata rename constraint asset_metadata_file_uuid_key to asset_metadata_asset_uuid_key;
