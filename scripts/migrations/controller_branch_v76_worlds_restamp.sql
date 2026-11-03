-- Editor worlds 144 and 238 were saved by the character controller branch at its own version 76, which only added the
-- CharacterController folder. After the merge, 76 means the rendering branch's step, so restamp them to 75 to run every
-- step from there; the controller step keeps the folder they have. Run it before the merged code opens them.
update public.game_editor_worlds
set gameworld_json = regexp_replace(gameworld_json, '"worldSchemaVersion":\s*76', '"worldSchemaVersion": 75')
where game_id in (144, 238)
  and gameworld_json like '%"CharacterController"%'
  and gameworld_json not like '%ambientOcclusionIntensity%';
