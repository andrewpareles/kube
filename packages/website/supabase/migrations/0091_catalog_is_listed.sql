-- A listed item is on the Catalog page and can be bought. An unlisted one is neither, but anyone with its id can still use it.
alter table public.catalog_metadata rename column is_public to is_listed;
alter policy "Anyone can read public catalog items" on public.catalog_metadata rename to "Anyone can read listed catalog items";

-- It names the column in its body, so it is made again with only the name changed.
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
    if not found or not item.is_listed then
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
