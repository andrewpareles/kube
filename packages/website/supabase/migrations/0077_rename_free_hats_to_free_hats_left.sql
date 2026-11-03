-- Named for what it counts: the free hats someone has left.
alter table public.user_profiles rename column free_hats to free_hats_left;
alter table public.user_profiles rename constraint user_profiles_free_hats_not_negative to user_profiles_free_hats_left_not_negative;

-- Both name the column in their bodies, so each is made again with only the name changed.
create or replace function public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_credits bigint)
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
      credits = user_profiles.credits + input_credits,
      free_hats_left = case when user_profiles.last_granted_invoice_id is null then 1 else user_profiles.free_hats_left end
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

create or replace function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint, input_price_in_credits bigint) returns bigint
language plpgsql
set search_path to ''
as $$
declare
    item public.catalog_metadata;
    buyer_balance bigint;
    buyer_free_hats_left integer;
    charged_price bigint;
begin
    select * into item from public.catalog_metadata where catalog_item_id = input_catalog_item_id for share;
    if not found or not item.is_public then
        raise exception 'This item is not for sale.';
    end if;

    select credits, free_hats_left into buyer_balance, buyer_free_hats_left from public.user_profiles where id = input_buyer_id for update;

    charged_price := case when buyer_free_hats_left > 0 then 0 else item.price_in_credits end;
    if charged_price <> input_price_in_credits then
        raise exception 'The price of this item changed. Check the new price and try again.';
    end if;

    if exists (select 1 from public.user_owned_catalog_items where profile_id = input_buyer_id and catalog_item_id = input_catalog_item_id) then
        raise exception 'You already own this item.';
    end if;

    if buyer_balance < charged_price then
        return null;
    end if;

    update public.user_profiles
    set credits = credits - charged_price,
        free_hats_left = free_hats_left - case when buyer_free_hats_left > 0 and item.price_in_credits > 0 then 1 else 0 end
    where id = input_buyer_id
    returning credits into buyer_balance;

    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_in_credits)
    values (input_buyer_id, input_catalog_item_id, charged_price);

    return buyer_balance;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint, bigint) from public, anon, authenticated;
