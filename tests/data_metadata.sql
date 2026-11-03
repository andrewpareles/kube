-- Run against a migrated database. Every test row is rolled back, including when an assertion fails.
begin;

do $$
declare
    test_uploader_id uuid;
    test_data_type public.data_type;
    inserted_row public.data_metadata%rowtype;
    first_data_id bigint;
begin
    if exists (
        select 1 from information_schema.columns
        where table_schema = 'public' and table_name = 'data_metadata' and column_name in ('name', 'url')
    ) then
        raise exception 'Raw uploads must not store asset names or derived paths';
    end if;

    select id into strict test_uploader_id from public.user_profiles limit 1;

    -- Raw assets need only their type and uploader; IDs remain unique within each type.
    foreach test_data_type in array enum_range(null::public.data_type) loop
        insert into public.data_metadata (data_type, uploaded_by)
        values (test_data_type, test_uploader_id)
        returning * into inserted_row;
        if inserted_row.data_id is null or inserted_row.data_type is distinct from test_data_type then
            raise exception 'Asset ID allocation failed for %', test_data_type;
        end if;
        first_data_id := inserted_row.data_id;

        insert into public.data_metadata (data_type, uploaded_by)
        values (test_data_type, test_uploader_id)
        returning * into inserted_row;
        if inserted_row.data_id <> first_data_id + 1 then
            raise exception 'Asset IDs are not successive for %', test_data_type;
        end if;

        begin
            insert into public.data_metadata (data_type, data_id, uploaded_by)
            values (test_data_type, first_data_id, test_uploader_id);
            raise exception 'Duplicate asset ID was accepted for %', test_data_type;
        exception when unique_violation then null;
        end;

        delete from public.data_metadata where data_type = inserted_row.data_type and data_id = inserted_row.data_id;
        insert into public.data_metadata (data_type, uploaded_by)
        values (test_data_type, test_uploader_id)
        returning * into inserted_row;
        if inserted_row.data_id <= first_data_id + 1 then
            raise exception 'A deleted asset ID was handed out again for %', test_data_type;
        end if;
    end loop;
end;
$$;

rollback;

select 'Raw asset IDs and unnamed uploads passed; test rows rolled back.' as result;
