-- Remixing is gone. A remix that was never opened in the editor or published has no world of its own yet, only the ids of the
-- world it started from, so that world is copied to it first, the way the editor did when a remix was first opened.
insert into public.game_editor_worlds (game_id, gameworld_json)
select remix.game_id, source_world.gameworld_json
from public.game_metadata remix
join public.game_worlds source_world on source_world.game_id = remix.remixed_from_game_id and source_world.version_num = remix.remixed_from_version_num
join public.game_metadata source on source.game_id = remix.remixed_from_game_id
where (source.is_published or source.creator_id = remix.creator_id)
  and not exists (select 1 from public.game_editor_worlds editor_world where editor_world.game_id = remix.game_id)
  and not exists (select 1 from public.game_worlds own_world where own_world.game_id = remix.game_id);

alter table public.game_metadata drop column remixed_from_game_id, drop column remixed_from_version_num;
