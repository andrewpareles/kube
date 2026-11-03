-- Each table is named for whose rows it holds: a user's k-coin ledger, the games we pick, and a user's desktop play login.
alter table public.k_coin_ledger rename to user_k_coin_ledger;
alter table public.our_picks_games rename to game_our_picks;
alter table public.player_auth rename to user_desktop_play_auth;

-- A function names its tables in its body, so the two that use a renamed table are defined again with only the name changed.
create or replace function public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid) returns bigint
language plpgsql
as $$
declare
  new_balance bigint;
begin
  select user_profile_row.k_coins into new_balance
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id
  for no key update;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;

  -- Answers with the balance as it stands now rather than the one this request left behind, which
  -- anything spent since has moved on from. The account has to match too: a request id belonging to
  -- somebody else is not this movement, and falls through to the insert, where the unique
  -- constraint refuses it.
  if exists (
    select 1
    from public.user_k_coin_ledger k_coin_ledger_row
    where k_coin_ledger_row.request_id = input_request_id
      and k_coin_ledger_row.profile_id = input_profile_id
  ) then
    return new_balance;
  end if;

  -- A spend that would take the balance below zero fails the check constraint here, which is what
  -- the caller catches to mean there was not enough.
  update public.user_profiles
  set k_coins = k_coins + input_amount
  where id = input_profile_id
  returning k_coins into new_balance;

  insert into public.user_k_coin_ledger (profile_id, amount, balance_after, reason, game_id, request_id)
  values (input_profile_id, input_amount, new_balance, input_reason, input_game_id, input_request_id);

  return new_balance;
end;
$$;

create or replace function public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) returns void
language plpgsql
as $$
begin
  -- Lock the same profile row as the website launch transaction so leave cleanup cannot race a new launch.
  perform 1
  from public.user_profiles
  where id = input_profile_id
  for update;

  -- Delete normal leaves and logout leaves, but do nothing if a newer Play launch replaced this connection's auth hash.
  delete from public.running_player_sessions
  where profile_id = input_profile_id
    and request_id = input_request_id
    and not exists (
      select 1
      from public.user_desktop_play_auth
      where profile_id = input_profile_id
        and play_auth_hash is not null
        and play_auth_hash <> input_play_auth_hash
    );
end;
$$;
