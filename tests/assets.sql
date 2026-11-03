begin;

-- These publications are rolled back; no storage objects or user profiles are created.
insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
select -9223372036854775000, id, 'Asset SQL Test old', 'Sound', gen_random_uuid() from public.user_profiles limit 1;
insert into public.asset_metadata(asset_id, version_num, uploaded_by, name, asset_type, asset_uuid)
select asset_id, 99, uploaded_by, 'Asset SQL Test latest', asset_type, gen_random_uuid()
from public.asset_metadata where asset_id = -9223372036854775000;

insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
select -9223372036854774998, id, 'Object SQL Test', 'Object', gen_random_uuid() from public.user_profiles limit 1;

insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
select -9223372036854774997, id, 'Material SQL Test', 'Material', gen_random_uuid() from public.user_profiles limit 1;

insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
select -9223372036854774996, id, 'Skybox SQL Test', 'Skybox', gen_random_uuid() from public.user_profiles limit 1;

do $$
begin
    if (select array_agg(version_num order by version_num) from public.asset_metadata where asset_id = -9223372036854775000) <> array[1,2]::bigint[] then
        raise exception 'Asset versions are not successive';
    end if;
    if (select count(*) from public.search_asset_metadata(input_query => 'Asset SQL Test', input_asset_type => 'Sound', input_offset => 0, input_limit => 10)) <> 1 then
        raise exception 'Search did not return exactly the latest revision';
    end if;
    if not exists (
        select 1 from public.search_asset_metadata('Asset SQL Test', 'Sound', 0, 10)
        where asset_id = -9223372036854775000 and asset_type = 'Sound' and version_num = 2
    ) then
        raise exception 'Search returned incorrect asset fields';
    end if;
    if exists(select 1 from public.search_asset_metadata('Asset SQL Test old', null, 0, 10)) then
        raise exception 'Search matched an obsolete revision';
    end if;
    if not exists (
        select 1 from public.search_asset_metadata('Object SQL Test', 'Object', 0, 10)
        where asset_id = -9223372036854774998 and asset_type = 'Object'
    ) then
        raise exception 'Object search filter did not return the published item';
    end if;
    if not exists (
        select 1 from public.search_asset_metadata('Material SQL Test', 'Material', 0, 10)
        where asset_id = -9223372036854774997 and asset_type = 'Material'
    ) then
        raise exception 'Material search filter did not return the published item';
    end if;
    if not exists (
        select 1 from public.search_asset_metadata('Skybox SQL Test', 'Skybox', 0, 10)
        where asset_id = -9223372036854774996 and asset_type = 'Skybox'
    ) then
        raise exception 'Skybox search filter did not return the published item';
    end if;
    begin
        insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
        select -9223372036854774999, uploaded_by, ' ', 'Sound', gen_random_uuid()
        from public.asset_metadata where asset_id = -9223372036854775000 limit 1;
        raise exception 'Blank asset name was accepted';
    exception when check_violation then null;
    end;
end;
$$;

do $$
begin
    update public.asset_metadata set import_count = import_count + 1 where asset_id = -9223372036854775000 and version_num = 2;
    update public.asset_metadata set import_count = import_count + 1 where asset_id = -9223372036854775000 and version_num = 2;
    if (select sum(import_count) from public.asset_metadata where asset_id = -9223372036854775000) <> 2 then
        raise exception 'Asset imports were not counted';
    end if;
    begin
        update public.asset_metadata set import_count = -1 where asset_id = -9223372036854775000 and version_num = 1;
        raise exception 'A negative import count was accepted';
    exception when check_violation then null;
    end;
end;
$$;

set local role anon;
do $$
begin
    if (select count(*) from public.asset_metadata where asset_id = -9223372036854775000) <> 2 then
        raise exception 'Public asset revisions are not readable';
    end if;
    if (select count(*) from public.search_asset_metadata('Asset SQL Test latest', null, 0, 10)) <> 1 then
        raise exception 'Public search is not readable';
    end if;
    begin
        delete from public.asset_metadata where asset_id = -9223372036854775000;
        raise exception 'Anonymous asset deletion succeeded';
    exception when insufficient_privilege then null;
    end;
end;
$$;
reset role;
set local role authenticated;
do $$
begin
    if (select count(*) from public.asset_metadata where asset_id = -9223372036854775000) <> 2 then
        raise exception 'Authenticated asset reads failed';
    end if;
    begin
        update public.asset_metadata set name = 'Changed' where asset_id = -9223372036854775000;
        raise exception 'Direct asset update succeeded';
    exception when insufficient_privilege then null;
    end;
    begin
        insert into public.asset_metadata(asset_id, uploaded_by, name, asset_type, asset_uuid)
        select -9223372036854774999, uploaded_by, 'Changed', asset_type, gen_random_uuid()
        from public.asset_metadata where asset_id = -9223372036854775000 limit 1;
        raise exception 'Direct asset insertion succeeded';
    exception when insufficient_privilege then null;
    end;
end;
$$;
reset role;
rollback;
