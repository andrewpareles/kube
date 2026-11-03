create or replace function public.delete_running_player_session_unless_play_auth_changed(
  input_profile_id uuid,
  input_request_id text,
  input_play_auth_hash text
)
returns void
language plpgsql
security invoker
as $$
begin
  -- Lock the same profile row as the website launch transaction so leave cleanup cannot race a new launch.
  perform 1
  from public.profiles
  where id = input_profile_id
  for update;

  -- Delete normal leaves and logout leaves, but do nothing if a newer Play launch replaced this connection's auth hash.
  delete from public.running_player_sessions
  where profile_id = input_profile_id
    and request_id = input_request_id
    and not exists (
      select 1
      from public.player_auth
      where profile_id = input_profile_id
        and play_auth_hash is not null
        and play_auth_hash <> input_play_auth_hash
    );
end;
$$;

revoke all on function public.delete_running_player_session_unless_play_auth_changed(uuid, text, text) from public;
grant execute on function public.delete_running_player_session_unless_play_auth_changed(uuid, text, text) to service_role;
