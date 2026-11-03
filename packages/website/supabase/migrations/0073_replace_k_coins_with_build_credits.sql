-- K coins are gone. Catalog items are bought with build credits, every item costs 100, and the daily reward and plans
-- only give build credits.
alter table public.catalog_metadata rename column price_k_coins to price_build_credits;
alter table public.catalog_metadata alter column price_build_credits set default 100;
update public.catalog_metadata set price_build_credits = 100;
alter table public.user_owned_catalog_items rename column price_paid_k_coins to price_paid_build_credits;

-- build_credits can go below zero after a chat, so the check here is what stops a purchase the buyer can't afford. It
-- answers with null then, and with the buyer's balance after a purchase.
drop function public.buy_catalog_item(uuid, bigint, bigint);
create function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint, input_price_build_credits bigint) returns bigint
language plpgsql
set search_path to ''
as $$
declare
    item public.catalog_metadata;
    buyer_balance bigint;
begin
    select * into item from public.catalog_metadata where catalog_item_id = input_catalog_item_id for share;
    if not found or not item.is_public then
        raise exception 'This item is not for sale.';
    end if;
    if item.price_build_credits <> input_price_build_credits then
        raise exception 'The price of this item changed. Check the new price and try again.';
    end if;

    perform 1 from public.user_profiles where id in (input_buyer_id, item.creator_id) order by id for update;

    if exists (select 1 from public.user_owned_catalog_items where profile_id = input_buyer_id and catalog_item_id = input_catalog_item_id) then
        raise exception 'You already own this item.';
    end if;

    select build_credits into buyer_balance from public.user_profiles where id = input_buyer_id;
    if buyer_balance < item.price_build_credits then
        return null;
    end if;

    update public.user_profiles set build_credits = build_credits - item.price_build_credits where id = input_buyer_id returning build_credits into buyer_balance;
    update public.user_profiles set build_credits = build_credits + item.price_build_credits where id = item.creator_id;

    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_build_credits)
    values (input_buyer_id, input_catalog_item_id, item.price_build_credits);

    return buyer_balance;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint, bigint) from public, anon, authenticated;

drop function public.claim_daily_reward(uuid, bigint, bigint, integer);
create function public.claim_daily_reward(input_profile_id uuid, input_build_credits bigint, input_min_hours integer)
returns table(build_credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  if input_build_credits is null or input_min_hours is null or input_build_credits < 0 or input_min_hours < 1 then
    raise exception 'claim_daily_reward: bad arguments (% build_credits, % hours)', input_build_credits, input_min_hours;
  end if;

  update public.user_profiles
  set daily_reward_claimed_at = now(),
      build_credits = user_profiles.build_credits + input_build_credits
  where id = input_profile_id
    and input_build_credits <> 0
    and (daily_reward_claimed_at is null or daily_reward_claimed_at <= now() - make_interval(hours => input_min_hours));

  did_grant := found;

  return query
  select user_profile_row.build_credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;
revoke execute on function public.claim_daily_reward(uuid, bigint, integer) from public, anon, authenticated;

drop function public.grant_invoice(uuid, text, bigint, bigint);
create function public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_build_credits bigint)
returns table(build_credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  if input_invoice_id is null or input_build_credits is null or input_build_credits < 0 then
    raise exception 'grant_invoice: bad arguments (% invoice, % build_credits)', input_invoice_id, input_build_credits;
  end if;

  update public.user_profiles
  set last_granted_invoice_id = input_invoice_id,
      build_credits = user_profiles.build_credits + input_build_credits
  where id = input_profile_id
    and last_granted_invoice_id is distinct from input_invoice_id;

  did_grant := found;

  return query
  select user_profile_row.build_credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;
revoke execute on function public.grant_invoice(uuid, text, bigint) from public, anon, authenticated;

drop function public.change_k_coins(uuid, bigint, text, bigint, uuid);
drop table public.user_k_coin_ledger;
alter table public.user_profiles drop column k_coins;
