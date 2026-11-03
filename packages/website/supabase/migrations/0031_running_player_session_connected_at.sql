-- A running_player_sessions row does two jobs at once. It is the ticket the game server checks in
-- validate_play_auth before letting a client into the world, and it is the record the website reads
-- to show who is in a server. The ticket has to exist before the client connects, so the website
-- writes the row the moment Play is pressed. That meant a player who pressed Play and never actually
-- launched the app - no client installed, deeplink dismissed, launch failed - showed up in the
-- server's player list and in the game's player count forever, and held a seat forever.
--
-- connected_at splits the two jobs apart. The row on its own is still the ticket, so the seat is
-- held from the moment Play is pressed, but only a row the game server has stamped counts as someone
-- actually being in the game. reserved_at gives an unstamped row an age, so a reservation can run
-- out: a player who has not reached the game in time has not joined, and their row is deleted by the
-- next launch that looks at that server. If their app opens after that, the missing row is a missing
-- ticket and the game server tells them to launch from the website again.
alter table public.running_player_sessions
  add column reserved_at timestamp with time zone not null default now(),
  add column connected_at timestamp with time zone;

-- Every row that exists right now belongs to a player the old code already counted as present, and
-- there is no way to tell from here which of them are real. Treating them all as connected keeps
-- current players visible and in their seats across this migration. Any phantoms among them are no
-- worse off than they were before it, and they wash out on their own: a row goes away when that
-- player presses Play again, and when the deployment terminates the row is cascade deleted.
update public.running_player_sessions
set connected_at = now();

-- Finding the expired reservations for one server is a read that runs inside every launch.
create index running_player_sessions_unconnected_idx
on public.running_player_sessions (request_id, reserved_at)
where connected_at is null;

-- The game server calls this once, in validate_play_auth, at the moment a client actually connects.
-- Stamping through an RPC rather than a PATCH from the game server keeps connected_at and
-- reserved_at on the same clock - the database's - so a game server whose clock has drifted cannot
-- write a connected_at that compares strangely against the reserved_at next to it.
--
-- Returning whether a row was updated lets the caller use this as the ticket check too, so admission
-- stays a single round trip: no row means no reservation, which means no entry.
create or replace function public.claim_running_player_session(
  input_profile_id uuid,
  input_request_id text
)
returns boolean
language plpgsql
security invoker
as $$
declare
  updated_count integer;
begin
  update public.running_player_sessions
  set connected_at = now()
  where profile_id = input_profile_id
    and request_id = input_request_id;

  get diagnostics updated_count = row_count;
  return updated_count > 0;
end;
$$;

revoke all on function public.claim_running_player_session(uuid, text) from public;
grant execute on function public.claim_running_player_session(uuid, text) to service_role;
