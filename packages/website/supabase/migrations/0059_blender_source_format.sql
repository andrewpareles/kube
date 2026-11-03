-- An editable model source is the .blend a Blender run saved, or the .glb someone uploaded, which Blender opens on its first edit.
alter table public.blender_metadata add column source_format text not null default 'blend' constraint blender_metadata_source_format_check check (source_format in ('blend', 'glb'));
