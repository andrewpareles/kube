-- Material catalog items stored the bare material, as {"Material": materialInfo}. They now keep the world schema version they
-- were published at, as {"Material": {"worldSchemaVersion": 84, "material": materialInfo}}, and are migrated as they are read
-- like the other catalog items. Every existing row already has the shape MaterialInfo has at version 84, so all are stamped 84.
update public.game_object_catalog
set data_json = jsonb_build_object('Material', jsonb_build_object('worldSchemaVersion', 84, 'material', data_json -> 'Material'))
where catalog_item_type = 'Material'
  and not (data_json -> 'Material' ? 'worldSchemaVersion');
