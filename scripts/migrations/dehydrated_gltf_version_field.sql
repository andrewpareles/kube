-- Catalog meshes kept their dehydrated glTF version beside the mesh, as {"Mesh": [dehydratedGltf, version]}, and it now
-- lives in the mesh, as {"Mesh": {..., "version": version}}. This only moves the number; the code migrates older versions
-- as it reads them. Run it when the dehydrated glTF version 5 code deploys: code before that reads only the old shape.
update public.game_object_catalog
set data_json = jsonb_build_object('Mesh', (data_json #> '{Mesh,0}') || jsonb_build_object('version', data_json #> '{Mesh,1}'))
where catalog_item_type = 'Mesh'
  and jsonb_typeof(data_json -> 'Mesh') = 'array';
