-- A catalog item is public or private. A private item stays out of the catalog and can't be bought, and anyone who already
-- owns it keeps it.
alter table public.catalog_metadata rename column is_for_sale to is_public;
alter policy "Catalog items for sale are public" on public.catalog_metadata rename to "Anyone can read public catalog items";

-- The buyer names the price they were shown, so a price changed since then refuses the purchase instead of charging it.
drop function public.buy_catalog_item(uuid, bigint);
create function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint, input_price_k_coins bigint) returns bigint
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
    if item.price_k_coins <> input_price_k_coins then
        raise exception 'The price of this item changed. Check the new price and try again.';
    end if;

    -- Both accounts are locked in the same order every time, so two people buying from each other at once can't deadlock.
    perform 1 from public.user_profiles where id in (input_buyer_id, item.creator_id) order by id for update;

    if exists (select 1 from public.user_owned_catalog_items where profile_id = input_buyer_id and catalog_item_id = input_catalog_item_id) then
        raise exception 'You already own this item.';
    end if;

    -- A spend below zero fails change_k_coins' check constraint, which is what the website reads as not enough K coins.
    if item.price_k_coins > 0 then
        perform public.change_k_coins(input_buyer_id, -item.price_k_coins, 'catalog_purchase', null::bigint, gen_random_uuid());
        perform public.change_k_coins(item.creator_id, item.price_k_coins, 'catalog_sale', null::bigint, gen_random_uuid());
    end if;

    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_k_coins)
    values (input_buyer_id, input_catalog_item_id, item.price_k_coins);

    select k_coins into buyer_balance from public.user_profiles where id = input_buyer_id;
    return buyer_balance;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint, bigint) from public, anon, authenticated;
