-- Give existing assets the type names requested for the initial catalog.
alter table public.data_metadata add column name text;

update public.data_metadata
set name = case data_type
    when 'sound_data' then 'sound'
    when 'texture_data' then 'texture'
    when 'mesh_data' then 'mesh'
end;

alter table public.data_metadata alter column name set not null;

-- Keep upload bookkeeping in the existing trigger. Explicit names are preserved, and uploads
-- without a name get the same type name used for the initial catalog.
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

    if new.url is null or new.url = '' then
        new.url := format('%s/%s', new.data_type::text, new.data_id::text);
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
