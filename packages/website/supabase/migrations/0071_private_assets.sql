-- An asset is public or private as a whole, so every revision carries the same visibility. Every existing asset was
-- public. New revisions always say which they are.
alter table public.asset_metadata add column is_public boolean;
update public.asset_metadata set is_public = true;
alter table public.asset_metadata alter column is_public set not null;

-- Anyone can read public assets, and people can read their own private ones.
drop policy "Assets are public" on public.asset_metadata;
create policy "Public assets and your own" on public.asset_metadata
as permissive
for select
to anon, authenticated
using (is_public or (select auth.uid()) = uploaded_by);

-- A new revision sets the visibility of the whole asset.
create or replace function public.set_asset_version()
returns trigger
language plpgsql
set search_path = ''
as $$
declare original public.asset_metadata;
begin
    perform pg_advisory_xact_lock(hashtextextended('asset_metadata:' || new.asset_id::text, 0));
    select * into original from public.asset_metadata where asset_id = new.asset_id order by version_num limit 1;
    if found and (original.uploaded_by <> new.uploaded_by or original.asset_type <> new.asset_type) then
        raise exception 'Asset publisher and type cannot change';
    end if;
    update public.asset_metadata set is_public = new.is_public where asset_id = new.asset_id and is_public <> new.is_public;
    select coalesce(max(version_num), 0) + 1 into new.version_num
    from public.asset_metadata where asset_id = new.asset_id;
    return new;
end;
$$;

-- Search finds public assets, and the private ones of the owner it is given.
drop function public.search_asset_metadata(text, public.asset_type, integer, integer);
create function public.search_asset_metadata(
    input_query text,
    input_asset_type public.asset_type,
    input_owner_id uuid,
    input_offset integer,
    input_limit integer
)
returns table(asset_id bigint, version_num bigint, name text, created_at timestamp with time zone, asset_type public.asset_type, is_public boolean)
language sql
stable
set search_path = ''
as $$
    select latest.asset_id, latest.version_num, latest.name, latest.created_at, latest.asset_type, latest.is_public
    from (
        select distinct on (asset.asset_id) asset.* from public.asset_metadata asset
        order by asset.asset_id, asset.version_num desc
    ) latest
    where (latest.is_public or latest.uploaded_by = input_owner_id)
      and (input_asset_type is null or latest.asset_type = input_asset_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.asset_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;
revoke all on function public.search_asset_metadata(text, public.asset_type, uuid, integer, integer) from public;
grant all on function public.search_asset_metadata(text, public.asset_type, uuid, integer, integer) to anon, authenticated, service_role;
