-- Every asset is public again. The ones that were private keep a note of it at the start of their name.
update public.asset_metadata set name = '[priv] - ' || name where not is_public;

drop policy "Public assets and your own" on public.asset_metadata;
create policy "Assets are public" on public.asset_metadata
as permissive
for select
to anon, authenticated
using (true);

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
    select coalesce(max(version_num), 0) + 1 into new.version_num
    from public.asset_metadata where asset_id = new.asset_id;
    return new;
end;
$$;

drop function public.search_asset_metadata(text, public.asset_type, uuid, integer, integer);
create function public.search_asset_metadata(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer)
returns table(asset_id bigint, version_num bigint, name text, created_at timestamp with time zone, asset_type public.asset_type)
language sql
stable
set search_path = ''
as $$
    select latest.asset_id, latest.version_num, latest.name, latest.created_at, latest.asset_type
    from (
        select distinct on (asset.asset_id) asset.* from public.asset_metadata asset
        order by asset.asset_id, asset.version_num desc
    ) latest
    where (input_asset_type is null or latest.asset_type = input_asset_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.asset_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;
revoke all on function public.search_asset_metadata(text, public.asset_type, integer, integer) from public;
grant all on function public.search_asset_metadata(text, public.asset_type, integer, integer) to anon, authenticated, service_role;

alter table public.asset_metadata drop column is_public;
