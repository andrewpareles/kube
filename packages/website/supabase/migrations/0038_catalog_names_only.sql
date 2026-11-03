alter table public.data_metadata drop column name;

drop function public.search_game_object_catalog(text, public.catalog_type, integer, integer);
alter table public.game_object_catalog drop column description;

-- Search the latest catalog names after removing descriptions.
create function public.search_game_object_catalog(input_query text, input_catalog_type public.catalog_type, input_offset integer, input_limit integer)
returns table(catalog_id bigint, version_num bigint, name text, created_at timestamptz, catalog_type public.catalog_type)
language sql stable security invoker set search_path = '' as $$
    select latest.catalog_id, latest.version_num, latest.name, latest.created_at, latest.catalog_type
    from (
        select distinct on (entry.catalog_id) entry.* from public.game_object_catalog entry
        order by entry.catalog_id, entry.version_num desc
    ) latest
    where (input_catalog_type is null or latest.catalog_type = input_catalog_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.catalog_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;
revoke all on function public.search_game_object_catalog(text, public.catalog_type, integer, integer) from public;
grant execute on function public.search_game_object_catalog(text, public.catalog_type, integer, integer) to anon, authenticated, service_role;
