update public.asset_metadata
set asset_type = 'Sfx', data_json = jsonb_build_object('Sfx', data_json -> 'Sound')
where asset_type = 'Sound' and asset_id >= 239;
