-- K-coins are the site's currency: one balance per account, spendable anywhere on kube.new rather
-- than inside a single game. The balance lives on user_profiles because it is one number per
-- account and every page that already knows who is signed in reads that row anyway. What a balance
-- is made of lives in its own table, for the reasons 0030 gives about chat_usage: a balance says
-- how much you have and can never say why, and history that was not written down cannot be
-- recovered later.

-- editor_ai_credits was added straight to the database and never wired to anything - no code, no
-- function, no trigger reads or writes it, and 76 of the 77 rows are still 0. It is dropped rather
-- than renamed into k_coins, because metering what the AI editor costs and paying for things on
-- the site are two different ideas, and renaming would also hand the one account that has 100 of
-- them a balance nobody meant to give it. A later credits column can be added in one line.
alter table public.user_profiles drop column editor_ai_credits;

-- The last thing that filled in a name for an account that arrived without one. 0026 moved names
-- to the sign up form and said an account with no name should fail outright rather than be given
-- one, but this trigger was left behind and kept handing out Guest<public_id>. It has not fired
-- since Feb 2026, and every account on the site signed up with email, so nothing reaches it.
-- NOTE: turning on a social login later will need handle_new_user to get a name from somewhere,
-- because without this trigger an insert with no username now fails the not-null constraint and
-- takes the whole sign up down with it. That is the behaviour 0026 asked for.
drop trigger set_default_profile_username on public.user_profiles;
drop function public.set_default_profile_username();

-- not null because for a balance there is no difference between having none and not knowing, and
-- a nullable column makes every read site write "?? 0". The check is what actually stops a spend
-- going past zero - the balance cannot be talked into being negative by a bug in a caller.
alter table public.user_profiles
add column k_coins bigint not null default 0
constraint user_profiles_k_coins_not_negative check (k_coins >= 0);

-- Nobody but the server writes money. user_profiles is the one table that lets a signed-in user
-- update their own row, which was harmless while the row held a name and an email, and is not
-- harmless now that it holds a balance. Nothing on the site has ever updated this table - profile
-- rows are made by handle_new_user, which is security definer and so is unaffected - so taking the
-- privilege away removes a write path that was never used.
revoke update on public.user_profiles from anon, authenticated;

-- The policy that used to allow it goes with the privilege, rather than being left behind as a
-- rule that can never be reached.
drop policy "Users can update own profile" on public.user_profiles;

-- One row per movement, in or out. Kept separate from the balance so that deleting a game, or
-- handing it to someone else, cannot rewrite what somebody was charged.
create table public.k_coin_ledger (
  id bigint generated always as identity primary key,
  -- Stamped from the caller at write time, never derived by joining through something that can
  -- change hands later.
  profile_id uuid not null
    references public.user_profiles (id)
    on delete cascade,
  -- Positive is earned, negative is spent. A row that moves nothing is a bug in the caller, not a
  -- thing worth recording.
  amount bigint not null constraint k_coin_ledger_amount_not_zero check (amount <> 0),
  -- What the balance became once this row was applied, so one row explains itself without adding
  -- up every row before it.
  balance_after bigint not null,
  -- Why it moved, in the caller's words. Left as free text while we do not yet know what earns
  -- and spends K-coins; it becomes an enum once that list settles.
  reason text not null,
  -- Which game it happened in, for attribution only. The game can go; the record stays.
  game_id bigint
    references public.game_metadata (game_id)
    on delete set null,
  -- The caller's id for this one movement. Unique, so a request that is sent twice - a retry after
  -- a reply went missing, a double-pressed button - cannot pay or charge twice. Same guard as
  -- chat_usage.call_id.
  request_id uuid not null unique,
  created_at timestamp with time zone not null default now()
);

-- "What has this account earned and spent, most recent first" is the query this table exists for.
create index k_coin_ledger_profile_created_idx on public.k_coin_ledger (profile_id, created_at);

alter table public.k_coin_ledger enable row level security;

create policy "Service role only" on public.k_coin_ledger
  as permissive for all to service_role using (true) with check (true);

-- The only way K-coins move. It changes the balance and writes the row that explains it in one
-- transaction, so the two can never disagree, and returns the new balance so the caller does not
-- have to read the row back.
create function public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid)
returns bigint
language plpgsql
as $$
declare
  new_balance bigint;
begin
  -- A request that already went through gives back what it gave the first time and changes
  -- nothing. This is what makes the call safe to retry.
  select k_coin_ledger_row.balance_after into new_balance
  from public.k_coin_ledger k_coin_ledger_row
  where k_coin_ledger_row.request_id = input_request_id;

  if found then
    return new_balance;
  end if;

  -- Reading and writing the balance in one statement takes a lock on the row, so two calls for the
  -- same account queue up behind each other instead of both starting from the same old number.
  -- A spend that would take the balance below zero fails the check constraint here, which is what
  -- the caller catches to mean there was not enough.
  update public.user_profiles
  set k_coins = k_coins + input_amount
  where id = input_profile_id
  returning k_coins into new_balance;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;

  insert into public.k_coin_ledger (profile_id, amount, balance_after, reason, game_id, request_id)
  values (input_profile_id, input_amount, new_balance, input_reason, input_game_id, input_request_id);

  return new_balance;
end;
$$;

-- Only the server moves coins. Creating a function hands execute to everyone by default, and
-- Supabase grants it to the browser's two keys on top of that, so all three have to be taken away
-- or the revoke above is the only thing standing between a signed-in user and their own balance.
-- The website connects as the owner, which always keeps execute.
revoke execute on function public.change_k_coins(uuid, bigint, text, bigint, uuid) from public, anon, authenticated;
