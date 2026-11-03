-- How many times each version of an asset has been added to a game from Assets. An asset's total is the sum over its versions.
alter table public.asset_metadata add column import_count bigint not null default 0 constraint asset_metadata_import_count_check check (import_count >= 0);
