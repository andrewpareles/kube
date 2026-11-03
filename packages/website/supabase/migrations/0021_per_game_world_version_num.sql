-- game_worlds.version_num was designed to be per-game: migration 0002 added a before-insert
-- trigger that assigns max(version_num) + 1 within the game, and migration 0017 kept the
-- set_gameworld_version function up to date through the table renames. The trigger itself is no
-- longer attached, so inserts fall through to the global identity sequence and every game shares
-- one running counter. This re-attaches the trigger so each game numbers its worlds 1, 2, 3...
--
-- Notes:
-- - The identity default still fires first; the trigger then overwrites it, so explicit
--   version_num values in inserts are ignored. The database always assigns the version.
-- - Existing rows keep their historical numbers (renumbering would break the composite
--   references from running_servers/starting_servers); each game continues from its own max.

create or replace function public.set_gameworld_version()
returns trigger
language plpgsql
as $$
begin
  -- Serialize concurrent inserts for the same game so two saves cannot compute the same version.
  perform pg_advisory_xact_lock(new.game_id);
  new.version_num := coalesce(
    (select max(game_world_row.version_num) + 1 from public.game_worlds game_world_row where game_world_row.game_id = new.game_id),
    1
  );
  return new;
end;
$$;

drop trigger if exists gameworlds_version_trigger on public.game_worlds;
drop trigger if exists game_worlds_set_version_num on public.game_worlds;

create trigger game_worlds_set_version_num
before insert on public.game_worlds
for each row
execute function public.set_gameworld_version();
