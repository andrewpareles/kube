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

    return new;
end;
$$;
