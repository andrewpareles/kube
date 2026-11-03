-- An empty game server calls this before it asks Edgegap to stop it. It deletes the server's running_servers row, so the
-- website stops sending players to it, unless someone is in the server or has a seat reserved in it. It takes the same
-- game_metadata lock as a Play launch. It returns true when the server has no row, which means it is safe to stop.
create or replace function public.delete_running_server_if_empty(input_request_id text)
returns boolean
language plpgsql
security invoker
as $$
declare
  server_game_id bigint;
begin
  -- Promotion swaps the starting row for the running row in one transaction, so starting_servers must be checked first.
  if exists (
    select 1
    from public.starting_servers
    where request_id = input_request_id
  ) then
    return false;
  end if;

  select game_id into server_game_id
  from public.running_servers
  where request_id = input_request_id;
  if not found then
    return true;
  end if;

  perform 1
  from public.game_metadata
  where game_id = server_game_id
  for update;

  -- A reservation nobody has connected with yet counts for 30 seconds, the website's RESERVATION_GRACE_SECONDS.
  delete from public.running_servers
  where request_id = input_request_id
    and not exists (
      select 1
      from public.running_player_sessions
      where request_id = input_request_id
        and (connected_at is not null or reserved_at > now() - interval '30 seconds')
    );
  return found;
end;
$$;

revoke all on function public.delete_running_server_if_empty(text) from public;
grant execute on function public.delete_running_server_if_empty(text) to service_role;
