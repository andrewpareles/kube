-- A remix is a new game that starts from one exact saved world of another game. The pair sits on
-- the game rather than on its worlds, because the game as a whole is what came from somewhere:
-- every world its creator saves after that is their own.
--
-- Both columns point at one row of game_worlds together, so a remix can never name a version that
-- was never saved. Deleting that world, which is what deleting a game does to all of its worlds,
-- empties the pair instead of blocking the delete, and the check keeps the pair from ending up
-- half filled in.
alter table public.game_metadata
  add column remixed_from_game_id bigint,
  add column remixed_from_version_num bigint,
  add constraint game_metadata_remixed_from_fkey
    foreign key (remixed_from_game_id, remixed_from_version_num)
    references public.game_worlds (game_id, version_num)
    on delete set null,
  add constraint game_metadata_remixed_from_both_or_neither
    check ((remixed_from_game_id is null) = (remixed_from_version_num is null));

-- Deleting a game deletes every one of its worlds, and each of those has to find the remixes that
-- point at it. Without this index that is a full scan of game_metadata per world.
create index game_metadata_remixed_from_idx
on public.game_metadata (remixed_from_game_id, remixed_from_version_num);
