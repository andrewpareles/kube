-- Custom SQL migration file, put your code below! --
-- This only updates the database metadata. Existing Storage objects still need to move
-- from uploads/animation_frames_data/{data_id} to uploads/bone_animation_data/{data_id}
-- in the same rollout window as this migration.

alter type public.data_type rename value 'animation_frames_data' to 'bone_animation_data';

update public."uploads-metadata"
set url = format('%s/%s', data_type::text, data_id::text)
where data_type = 'bone_animation_data';
