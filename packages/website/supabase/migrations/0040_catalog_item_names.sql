-- The catalog is the collection; IDs and types describe individual catalog items.
drop function public.search_game_object_catalog(text, public.catalog_type, integer, integer);
alter type public.catalog_type rename to catalog_item_type;
alter table public.game_object_catalog rename column catalog_id to catalog_item_id;
alter table public.game_object_catalog rename column catalog_type to catalog_item_type;
alter sequence public.game_object_catalog_catalog_id_seq rename to game_object_catalog_catalog_item_id_seq;

alter function public.set_catalog_version() rename to set_catalog_item_version;
create or replace function public.set_catalog_item_version()
returns trigger language plpgsql set search_path = '' as $$
declare original public.game_object_catalog;
begin
    perform pg_advisory_xact_lock(hashtextextended('game_object_catalog:' || new.catalog_item_id::text, 0));
    select * into original from public.game_object_catalog where catalog_item_id = new.catalog_item_id order by version_num limit 1;
    if found and (original.uploaded_by <> new.uploaded_by or original.catalog_item_type <> new.catalog_item_type) then
        raise exception 'Catalog item publisher and type cannot change';
    end if;
    select coalesce(max(version_num), 0) + 1 into new.version_num
    from public.game_object_catalog where catalog_item_id = new.catalog_item_id;
    return new;
end;
$$;
alter trigger game_object_catalog_set_version on public.game_object_catalog rename to game_object_catalog_set_item_version;

-- Recreate search with the renamed input and result fields, retaining its public read permissions.
create function public.search_game_object_catalog(input_query text, input_catalog_item_type public.catalog_item_type, input_offset integer, input_limit integer)
returns table(catalog_item_id bigint, version_num bigint, name text, created_at timestamptz, catalog_item_type public.catalog_item_type)
language sql stable security invoker set search_path = '' as $$
    select latest.catalog_item_id, latest.version_num, latest.name, latest.created_at, latest.catalog_item_type
    from (
        select distinct on (catalog_item.catalog_item_id) catalog_item.* from public.game_object_catalog catalog_item
        order by catalog_item.catalog_item_id, catalog_item.version_num desc
    ) latest
    where (input_catalog_item_type is null or latest.catalog_item_type = input_catalog_item_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.catalog_item_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;
revoke all on function public.search_game_object_catalog(text, public.catalog_item_type, integer, integer) from public;
grant execute on function public.search_game_object_catalog(text, public.catalog_item_type, integer, integer) to anon, authenticated, service_role;
