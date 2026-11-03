-- Catalog meshes uploaded before the editor server's history was rebuilt are stamped version 4.
-- That payload is today's version 3 payload plus a root blender_id the current types do not have,
-- so the renderer's exact-version check is the only thing rejecting them.
-- Drop the stray field and restamp to 3. Older stamps stay stale; this migration does not upgrade them.
update public.game_object_catalog
set data_json = jsonb_set(
        jsonb_set(data_json, '{Mesh,0}', (data_json #> '{Mesh,0}') - 'blender_id'),
        '{Mesh,1}',
        '3'
    )
where catalog_item_type = 'Mesh'
  and data_json #>> '{Mesh,1}' = '4'
  -- Every such row carries blender_id null; refuse to discard one that points at a stored .blend.
  and coalesce(data_json #> '{Mesh,0,blender_id}', 'null'::jsonb) = 'null'::jsonb;
