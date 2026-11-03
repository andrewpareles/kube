delete from public."uploads-metadata"
where data_type = 'bone_animation_data';

alter type public.data_type rename to data_type_old;

create type public.data_type as enum ('mesh_data', 'texture_data', 'sound_data');

alter table public."uploads-metadata"
alter column data_type type public.data_type
using data_type::text::public.data_type;

drop type public.data_type_old;
