-- Custom SQL migration file, put your code below! --

create or replace function public.set_type_id()
returns trigger
language plpgsql
as $$
begin
    if new.data_id is null then
        -- lock per data type to avoid duplicate ids during concurrent inserts
        perform pg_advisory_xact_lock(hashtext(new.data_type::text));

        select coalesce(max(upload_row.data_id), 0) + 1
        into new.data_id
        from public.uploads upload_row
        where upload_row.data_type = new.data_type;
    end if;

    if new.url is null or new.url = '' then
        new.url := format('%s/%s', new.data_type::text, new.data_id::text);
    end if;

    return new;
end;
$$;

drop trigger if exists uploads_set_type_id on public.uploads;

create trigger uploads_set_type_id
before insert on public.uploads
for each row
execute function public.set_type_id();