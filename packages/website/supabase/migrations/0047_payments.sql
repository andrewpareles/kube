-- Everything needed to charge for the editor: a second currency, the daily giveaway, and a copy of
-- what Stripe says an account is paying for. It also fixes change_k_coins, which is already live
-- and which the daily giveaway calls.
--
-- There is no ledger table for build credits to match k_coin_ledger, because chat_usage is already one
-- row per model call and that is where nearly every credit goes. Recording the charge on that row
-- gives the same history without a second table, and leaves a spend as one atomic statement:
--
--   update user_profiles set build_credits = build_credits - $1 where id = $2 returning build_credits
--
-- What that gives up is the sum of the movements adding up to the balance, which k_coin_ledger can
-- still prove and this cannot.
--
-- The subscription gets a table of its own rather than columns on user_profiles. A signed-in user
-- can read their own user_profiles row, so anything put there reaches their browser; and a row that
-- only exists once somebody has subscribed lets every column be not null, where columns on the
-- profile would all have to be nullable for the accounts that never have.

-- --------------------------- the fix to what is already live ---------------------------

-- The guard that made a retry safe looked for an earlier run with a plain select and then wrote the
-- row, with nothing holding the account still in between. Two calls carrying the same request id at
-- the same moment both looked, both found nothing, and the second one met the unique constraint on
-- the way in - so a resend that should have answered with the balance raised instead. The balance
-- was never wrong, because that constraint is what stopped the second row; the caller just got an
-- error where it expected a number.
--
-- Taking the lock on the account before looking is what fixes it: the second call waits there, and
-- by the time it looks the first call's row is already written.
create or replace function public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid)
returns bigint
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
    from public.k_coin_ledger k_coin_ledger_row
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

  insert into public.k_coin_ledger (profile_id, amount, balance_after, reason, game_id, request_id)
  values (input_profile_id, input_amount, new_balance, input_reason, input_game_id, input_request_id);

  return new_balance;
end;
$$;

-- 0045 already took execute away from these three, and replacing a function keeps the privileges it
-- had, so this changes nothing today. It is here because the thing it guards against is silent: if
-- the argument list above ever drifts from 0045's by even one type, this stops being a replacement
-- and becomes a second function alongside it, carrying the execute-for-everyone that Postgres gives
-- every new function - and any signed-in account could then call it and pay itself.
revoke execute on function public.change_k_coins(uuid, bigint, text, bigint, uuid) from public, anon, authenticated;

-- --------------------------- the new columns ---------------------------

create type public.plan_id as enum ('pro', 'max');

-- One statement, so the table is locked and walked once rather than three times.
alter table public.user_profiles
-- not null because for a balance there is no difference between having none and not knowing, and a
-- nullable column makes every read site write "?? 0". Unlike k_coins there is no check keeping this
-- at or above zero: a charge is written after the model call already ran and AWS already billed us,
-- so it has to be recordable even when it takes the balance under. How far under is set by whatever
-- refuses to start a chat, not by the database.
add column build_credits bigint not null default 0,
-- Null until the account claims for the first time, which is a different thing from having claimed
-- at the start of time.
add column daily_reward_claimed_at timestamp with time zone,
-- The last invoice this account was paid out for. Only grant_invoice below is allowed to read or
-- write it, because the test and the write have to be the same statement to mean anything.
-- It lives here and not on user_subscriptions for two reasons. It sits on the same row as the
-- balance it guards, so one statement both tests it and pays out. And it has to outlive the
-- subscription: Stripe keeps delivering invoice events after somebody cancels, and a guard deleted
-- along with their subscription row would let a late one pay out again.
add column last_granted_invoice_id text;

-- What the account was charged for this one model call. Nullable, and null for every row written
-- before charging existed - which is a different thing from having been charged nothing.
-- The token counts beside it stay raw on purpose, as 0030 says: they are what AWS billed us, and
-- they have to survive a change to the rate or the margin. This column is the opposite - it is what
-- the user was charged at the moment they were charged, and it never moves again.
alter table public.chat_usage
add column credits_charged integer;

-- --------------------------- the subscription ---------------------------

-- A copy of what Stripe says an account is paying for. One row per account that has ever
-- subscribed. The row is kept when they cancel, with Stripe's own status written into it, so that if
-- they come back they are the same Stripe customer and keep their saved card and billing history.
-- So an account is on the free tier when it has no row, or when its status is not an active one.
-- There is no 'free' in plan_id, because not paying is not a plan.
-- Nothing here is decided locally: the webhook overwrites the row with whatever Stripe says the
-- subscription looks like at the moment we are told something changed. Stripe does not promise
-- events arrive in the order it made them, and its own advice is not to sort them by their
-- timestamp, because that timestamp is only good to the second and two events can share one. So the
-- handler never reads the state out of the event it was handed - it asks Stripe what the
-- subscription looks like now and writes that, and an event arriving late writes the same thing.
create table public.user_subscriptions (
  profile_id uuid primary key
    references public.user_profiles (id)
    on delete cascade,
  plan public.plan_id not null,
  -- Text and not an enum. Stripe adds new statuses on its own schedule, and an enum would throw on
  -- the webhook carrying one - the one moment we cannot afford to drop a message.
  status text not null,
  -- Who this account is in Stripe. Unique, because it is how the webhook finds the account.
  stripe_customer_id text not null unique,
  current_period_end timestamp with time zone not null
);

alter table public.user_subscriptions enable row level security;

-- Row level security only covers reads and writes of rows, so the privileges go too, the same as
-- 0046. This is what keeps stripe_customer_id out of the browser.
revoke all on public.user_subscriptions from anon, authenticated;
grant all on public.user_subscriptions to service_role;

create policy "Service role only" on public.user_subscriptions
  as permissive for all to service_role using (true) with check (true);

-- --------------------------- the daily giveaway ---------------------------

-- Answers with the balances whether or not it granted anything, so a page load never needs a second
-- read. How much to give and how long to wait are the caller's to decide, because those numbers
-- live in lib/pricing.ts.
-- input_profile_id has to come from the signed-in session on the server. The website connects as
-- the owner, so row level security does not apply inside here and nothing checks who is calling:
-- whoever passes an id decides whose account moves.
create function public.claim_daily_reward(input_profile_id uuid, input_k_coins bigint, input_build_credits bigint, input_min_hours integer)
returns table(k_coins bigint, build_credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  -- Every number here arrives as an argument and nothing else bounds them. This function only ever
  -- gives, so a negative amount would take money out, and a window of zero would let every page
  -- load through.
  if input_k_coins is null or input_build_credits is null or input_min_hours is null
    or input_k_coins < 0 or input_build_credits < 0 or input_min_hours < 1 then
    raise exception 'claim_daily_reward: bad arguments (% k_coins, % build_credits, % hours)', input_k_coins, input_build_credits, input_min_hours;
  end if;

  -- Stamping the row, testing how old the stamp is, and adding the credits all happen in one
  -- statement, which is what serializes two tabs opening together: the second waits on the row
  -- lock, then tests its where against the timestamp the first one wrote and matches nothing.
  -- A reward with nothing to give on either side is left unstamped, so it does not use up a window
  -- it got nothing out of.
  -- build_credits is written out in full on the right, because this function also returns a column of
  -- that name and a bare mention would be the returned value rather than what is in the row.
  update public.user_profiles
  set daily_reward_claimed_at = now(),
      build_credits = user_profiles.build_credits + input_build_credits
  where id = input_profile_id
    and (input_k_coins <> 0 or input_build_credits <> 0)
    and (daily_reward_claimed_at is null or daily_reward_claimed_at <= now() - make_interval(hours => input_min_hours));

  -- Read straight away, because the call below sets found as well.
  did_grant := found;

  -- K-coins still move through their own function, because they still have a ledger to write and
  -- that function is the only thing allowed to write it. A reward with no k-coins in it skips the
  -- call, because k_coin_ledger refuses a row that moves nothing.
  if did_grant and input_k_coins <> 0 then
    perform public.change_k_coins(input_profile_id, input_k_coins, 'daily_reward', null::bigint, gen_random_uuid());
  end if;

  -- granted says whether this call is the one that gave the money. The balances alone cannot say:
  -- every tab gets the same two numbers back, and a chat charge landing in between moves them.
  return query
  select user_profile_row.k_coins, user_profile_row.build_credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;

-- Same reason as change_k_coins: create hands execute to everyone, and Supabase adds the browser's
-- two keys on top, so an account could otherwise pay itself by calling this in a loop.
revoke execute on function public.claim_daily_reward(uuid, bigint, bigint, integer) from public, anon, authenticated;

-- --------------------------- paying out an invoice ---------------------------

-- The only way a paid invoice turns into money. Stripe sends the same event more than once - on its
-- own retries, and sometimes as two separate events for one payment - so the deciding question is
-- always "have we already paid out this invoice", and the answer has to be read and written in the
-- same statement or two deliveries arriving together both read the old answer.
--
-- That is why this is a function and not something the webhook handler does itself. A handler that
-- selects the last invoice id, compares it, and then updates would look correct and would pay twice
-- under exactly the delivery behaviour Stripe documents.
--
-- The k-coin side is keyed on the invoice as well, rather than on a fresh id, so it is guarded
-- twice: once by the statement below and once by k_coin_ledger's unique request id. build credits only
-- have the first guard, because they have no ledger. The difference shows up in one case: a
-- hand-triggered resend of an older invoice, after a newer one has been paid out, gets past the
-- statement below and would hand out build credits a second time, while the k-coins are refused. It
-- needs somebody to go and press resend on an old invoice in the Stripe dashboard.
create function public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_k_coins bigint, input_build_credits bigint)
returns table(k_coins bigint, build_credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  if input_invoice_id is null or input_k_coins is null or input_build_credits is null
    or input_k_coins < 0 or input_build_credits < 0 then
    raise exception 'grant_invoice: bad arguments (% invoice, % k_coins, % build_credits)', input_invoice_id, input_k_coins, input_build_credits;
  end if;

  -- Writing the invoice id and adding the credits in the statement that also tests the invoice id
  -- is the whole guard: the second delivery waits on the row lock, then tests its where against the
  -- id the first one wrote and matches nothing.
  -- build_credits is written out in full on the right, because this function also returns a column of
  -- that name and a bare mention would be the returned value rather than what is in the row.
  update public.user_profiles
  set last_granted_invoice_id = input_invoice_id,
      build_credits = user_profiles.build_credits + input_build_credits
  where id = input_profile_id
    and last_granted_invoice_id is distinct from input_invoice_id;

  did_grant := found;

  -- Made from the invoice id rather than at random, so the ledger's unique request id refuses a
  -- second payout for the same invoice however this function is reached.
  if did_grant and input_k_coins <> 0 then
    perform public.change_k_coins(input_profile_id, input_k_coins, 'subscription', null::bigint, md5('stripe_invoice:' || input_invoice_id)::uuid);
  end if;

  return query
  select user_profile_row.k_coins, user_profile_row.build_credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;

revoke execute on function public.grant_invoice(uuid, text, bigint, bigint) from public, anon, authenticated;
