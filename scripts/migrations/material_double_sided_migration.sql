-- Add the explicit material field to otherwise-current worlds and catalog payloads.
-- Keep older schema versions stale; this migration does not upgrade unrelated formats.
-- Existing materials remain single-sided. Restore original glTF settings separately when the source is available.
do $$
declare
    item record;
    object_entry record;
    segment_entry record;
    payload jsonb;
    world_json jsonb;
begin
    for item in
        select 'World' as kind, game_id as item_id, version_num, gameworld_json::jsonb as data_json
        from public.game_worlds where gameworld_json::jsonb->>'worldSchemaVersion' = '74'
        union all
        select catalog_item_type::text, catalog_item_id, version_num, data_json
        from public.game_object_catalog
        where (catalog_item_type = 'StoredObject' and data_json #>> '{StoredObject,worldSchemaVersion}' = '74')
           or (catalog_item_type = 'Mesh' and data_json #>> '{Mesh,1}' = '2')
    loop
        payload := item.data_json;
        if item.kind = 'Mesh' then
            for segment_entry in
                select value, ordinality from jsonb_array_elements(payload #> '{Mesh,0,dehydrated_meshes}') with ordinality
            loop
                if jsonb_typeof(segment_entry.value->'material_info') = 'object' then
                    payload := jsonb_set(payload,
                        array['Mesh','0','dehydrated_meshes',(segment_entry.ordinality - 1)::text,'material_info','isDoubleSided'], 'false');
                end if;
            end loop;
            payload := jsonb_set(payload, '{Mesh,1}', '3');
        else
            world_json := case when item.kind = 'World' then payload else payload->'StoredObject' end;
            for object_entry in select key, value from jsonb_each(world_json->'objectOfId') loop
                if jsonb_typeof(object_entry.value #> '{info,Material}') = 'object' then
                    world_json := jsonb_set(world_json,
                        array['objectOfId',object_entry.key,'info','Material','isDoubleSided'], 'false');
                end if;
            end loop;
            world_json := jsonb_set(world_json, '{worldSchemaVersion}', '75');
            payload := case when item.kind = 'World' then world_json else jsonb_set(payload, '{StoredObject}', world_json) end;
        end if;
        if item.kind = 'World' then
            update public.game_worlds set gameworld_json = payload::text
            where game_id = item.item_id and version_num = item.version_num;
        else
            update public.game_object_catalog set data_json = payload
            where catalog_item_id = item.item_id and version_num = item.version_num;
        end if;
    end loop;
end;
$$;
