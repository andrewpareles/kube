-- Build credits are just called credits.
alter table public.user_profiles rename column build_credits to credits;
alter table public.catalog_metadata rename column price_build_credits to price_in_credits;
alter table public.user_owned_catalog_items rename column price_paid_build_credits to price_paid_in_credits;

-- These three name the old columns in their arguments, results or bodies, so each is made again with only the names changed.
drop function public.buy_catalog_item(uuid, bigint, bigint);
create function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint, input_price_in_credits bigint) returns bigint
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
    if item.price_in_credits <> input_price_in_credits then
        raise exception 'The price of this item changed. Check the new price and try again.';
    end if;

    select credits into buyer_balance from public.user_profiles where id = input_buyer_id for update;

    if exists (select 1 from public.user_owned_catalog_items where profile_id = input_buyer_id and catalog_item_id = input_catalog_item_id) then
        raise exception 'You already own this item.';
    end if;

    if buyer_balance < item.price_in_credits then
        return null;
    end if;

    update public.user_profiles set credits = credits - item.price_in_credits where id = input_buyer_id returning credits into buyer_balance;

    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_in_credits)
    values (input_buyer_id, input_catalog_item_id, item.price_in_credits);

    return buyer_balance;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint, bigint) from public, anon, authenticated;

drop function public.claim_daily_reward(uuid, bigint, integer);
create function public.claim_daily_reward(input_profile_id uuid, input_credits bigint, input_min_hours integer)
returns table(credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  if input_credits is null or input_min_hours is null or input_credits < 0 or input_min_hours < 1 then
    raise exception 'claim_daily_reward: bad arguments (% credits, % hours)', input_credits, input_min_hours;
  end if;

  update public.user_profiles
  set daily_reward_claimed_at = now(),
      credits = user_profiles.credits + input_credits
  where id = input_profile_id
    and input_credits <> 0
    and (daily_reward_claimed_at is null or daily_reward_claimed_at <= now() - make_interval(hours => input_min_hours));

  did_grant := found;

  return query
  select user_profile_row.credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;
revoke execute on function public.claim_daily_reward(uuid, bigint, integer) from public, anon, authenticated;

drop function public.grant_invoice(uuid, text, bigint);
create function public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_credits bigint)
returns table(credits bigint, granted boolean)
language plpgsql
as $$
declare
  did_grant boolean;
begin
  if input_invoice_id is null or input_credits is null or input_credits < 0 then
    raise exception 'grant_invoice: bad arguments (% invoice, % credits)', input_invoice_id, input_credits;
  end if;

  update public.user_profiles
  set last_granted_invoice_id = input_invoice_id,
      credits = user_profiles.credits + input_credits
  where id = input_profile_id
    and last_granted_invoice_id is distinct from input_invoice_id;

  did_grant := found;

  return query
  select user_profile_row.credits, did_grant
  from public.user_profiles user_profile_row
  where user_profile_row.id = input_profile_id;

  if not found then
    raise exception 'No profile %', input_profile_id;
  end if;
end;
$$;
revoke execute on function public.grant_invoice(uuid, text, bigint) from public, anon, authenticated;
