-- Upload origin was not recorded before this migration. These historical classifications
-- are inferred from the upload batches and the material/skybox catalogs, as requested.
lock table public.data_metadata in access exclusive mode;

alter table public.data_metadata add column is_main boolean;

update public.data_metadata
set is_main = case
    when data_type in ('mesh_data', 'sound_data') then true
    -- These early image uploads had no accompanying mesh uploads.
    when data_type = 'texture_data' and data_id between 22 and 28 then true
    -- Standalone material maps and skyboxes; skybox IDs 392–424 have a source manifest.
    when data_type = 'texture_data' and data_id between 131 and 424 then true
    -- Textures uploaded in model import batches, including the September 7 model batch.
    when data_type = 'texture_data' and
        (data_id between 1 and 21 or data_id between 29 and 130 or data_id between 425 and 436) then false
end;

-- Unknown historical textures make the migration fail instead of silently guessing.
-- New uploads must explicitly supply their classification; there is no default.
alter table public.data_metadata alter column is_main set not null;
