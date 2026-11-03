-- The catalog is now called assets, and each of its items an asset.
alter type public.catalog_item_type rename to asset_type;
alter table public.game_object_catalog rename to game_object_assets;
alter table public.game_object_assets rename column catalog_item_id to asset_id;
alter table public.game_object_assets rename column catalog_item_type to asset_type;
alter table public.game_object_assets rename constraint game_object_catalog_pkey to game_object_assets_pkey;
alter table public.game_object_assets rename constraint game_object_catalog_data_type_check to game_object_assets_data_type_check;
alter table public.game_object_assets rename constraint game_object_catalog_name_check to game_object_assets_name_check;
alter table public.game_object_assets rename constraint game_object_catalog_uploaded_by_fkey to game_object_assets_uploaded_by_fkey;
alter index public.game_object_catalog_uploaded_by_idx rename to game_object_assets_uploaded_by_idx;
alter sequence public.game_object_catalog_catalog_item_id_seq rename to game_object_assets_asset_id_seq;
alter policy "Catalog is public" on public.game_object_assets rename to "Assets are public";

-- The version trigger names the table and its columns in its body, so it is defined again under the new names.
create function public.set_asset_version() returns trigger
language plpgsql
set search_path to ''
as $$
declare original public.game_object_assets;
begin
    perform pg_advisory_xact_lock(hashtextextended('game_object_assets:' || new.asset_id::text, 0));
    select * into original from public.game_object_assets where asset_id = new.asset_id order by version_num limit 1;
    if found and (original.uploaded_by <> new.uploaded_by or original.asset_type <> new.asset_type) then
        raise exception 'Asset publisher and type cannot change';
    end if;
    select coalesce(max(version_num), 0) + 1 into new.version_num
    from public.game_object_assets where asset_id = new.asset_id;
    return new;
end;
$$;
grant all on function public.set_asset_version() to anon, authenticated, service_role;
drop trigger game_object_catalog_set_item_version on public.game_object_assets;
create trigger game_object_assets_set_version before insert on public.game_object_assets for each row execute function public.set_asset_version();
drop function public.set_catalog_item_version();

create function public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer)
returns table(asset_id bigint, version_num bigint, name text, created_at timestamp with time zone, asset_type public.asset_type)
language sql
stable
set search_path to ''
as $$
    select latest.asset_id, latest.version_num, latest.name, latest.created_at, latest.asset_type
    from (
        select distinct on (asset.asset_id) asset.* from public.game_object_assets asset
        order by asset.asset_id, asset.version_num desc
    ) latest
    where (input_asset_type is null or latest.asset_type = input_asset_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.asset_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;
revoke all on function public.search_game_object_assets(text, public.asset_type, integer, integer) from public;
grant all on function public.search_game_object_assets(text, public.asset_type, integer, integer) to anon, authenticated, service_role;

-- Servers and the website from before this rename keep working until the release that uses the new names is out: they still
-- read and write the old table name through this view, and search through the old function, which reads the view.
-- 0055 removes both.
create view public.game_object_catalog with (security_invoker = true) as
select asset_id as catalog_item_id, version_num, name, created_at, uploaded_by, asset_type as catalog_item_type, data_json
from public.game_object_assets;
grant all on table public.game_object_catalog to service_role;
grant select on table public.game_object_catalog to anon, authenticated;
