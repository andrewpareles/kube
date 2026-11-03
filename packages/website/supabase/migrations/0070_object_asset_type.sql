alter type public.asset_type rename value 'StoredObject' to 'Object';

update public.asset_metadata
set data_json = jsonb_build_object('Object', data_json -> 'StoredObject')
where asset_type = 'Object';
