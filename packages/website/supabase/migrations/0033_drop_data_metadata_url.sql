-- Asset storage paths are derived from data_type and data_id by the uploader and game.
-- Check existing values before removing the duplicate field.
lock table public.data_metadata in access exclusive mode;

do $$
begin
    if exists (
        select 1 from public.data_metadata
        where url is distinct from format('%s/%s', data_type, data_id)
    ) then
        raise exception 'Cannot remove data_metadata.url while custom storage paths exist';
    end if;
end;
$$;

create or replace function public.set_type_id()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
    if new.data_id is null then
        perform pg_advisory_xact_lock(hashtext(new.data_type::text));

        select coalesce(max(data_row.data_id), 0) + 1
        into new.data_id
        from public.data_metadata data_row
        where data_row.data_type = new.data_type;
    end if;

    if new.name is null then
        new.name := case new.data_type
            when 'sound_data' then 'sound'
            when 'texture_data' then 'texture'
            when 'mesh_data' then 'mesh'
        end;
    end if;

    return new;
end;
$$;

alter table public.data_metadata drop column url;
