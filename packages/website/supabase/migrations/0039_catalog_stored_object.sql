alter table public.game_object_catalog drop constraint game_object_catalog_data_type_check;

-- The Object category stores the StoredObject variant of CatalogData.
update public.game_object_catalog
set data_json = jsonb_build_object('StoredObject', data_json -> 'Object')
where catalog_type = 'Object';

alter table public.game_object_catalog add constraint game_object_catalog_data_type_check check (
    jsonb_typeof(data_json) = 'object'
    and case when catalog_type = 'Object' then
        data_json ? 'StoredObject' and data_json - 'StoredObject' = '{}'::jsonb
    else
        data_json ? catalog_type::text and data_json - catalog_type::text = '{}'::jsonb
    end
);
