-- A file named by an integer id is uploaded before the row that records it, so its id is taken from the counter first. Only our
-- servers can take one.
create function public.next_data_id() returns bigint
language sql
security definer
set search_path = ''
as $$
    select nextval(pg_get_serial_sequence('public.data_metadata', 'data_id'))
$$;

create function public.next_catalog_item_id() returns bigint
language sql
security definer
set search_path = ''
as $$
    select nextval(pg_get_serial_sequence('public.catalog_metadata', 'catalog_item_id'))
$$;

create function public.next_blender_id() returns bigint
language sql
security definer
set search_path = ''
as $$
    select nextval(pg_get_serial_sequence('public.blender_metadata', 'blender_id'))
$$;

revoke execute on function public.next_data_id(), public.next_catalog_item_id(), public.next_blender_id() from public, anon, authenticated;
grant execute on function public.next_data_id(), public.next_catalog_item_id(), public.next_blender_id() to service_role;
