-- Match the catalog item type to its existing StoredObject payload variant.
alter type public.catalog_item_type rename value 'Object' to 'StoredObject';

-- Every catalog item now uses its type name as its payload key.
alter table public.game_object_catalog drop constraint game_object_catalog_data_type_check;
alter table public.game_object_catalog add constraint game_object_catalog_data_type_check check (
    jsonb_typeof(data_json) = 'object'
    and data_json ? catalog_item_type::text
    and data_json - catalog_item_type::text = '{}'::jsonb
);
