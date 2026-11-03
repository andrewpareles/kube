-- A profile wears one item it owns. Everyone starts out wearing the item their avatar already showed, the one they got most
-- recently.
alter table public.user_profiles
add column equipped_catalog_item_id bigint,
add constraint user_profiles_equipped_item_is_owned foreign key (id, equipped_catalog_item_id) references public.user_owned_catalog_items (profile_id, catalog_item_id);

update public.user_profiles
set equipped_catalog_item_id = latest.catalog_item_id
from (
    select distinct on (profile_id) profile_id, catalog_item_id
    from public.user_owned_catalog_items
    order by profile_id, purchased_at desc, catalog_item_id desc
) latest
where latest.profile_id = user_profiles.id;

-- The build credits a purchase costs are spent, not paid to the item's creator.
create or replace function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint, input_price_build_credits bigint) returns bigint
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

    select build_credits into buyer_balance from public.user_profiles where id = input_buyer_id for update;

    if exists (select 1 from public.user_owned_catalog_items where profile_id = input_buyer_id and catalog_item_id = input_catalog_item_id) then
        raise exception 'You already own this item.';
    end if;

    if buyer_balance < item.price_build_credits then
        return null;
    end if;

    update public.user_profiles set build_credits = build_credits - item.price_build_credits where id = input_buyer_id returning build_credits into buyer_balance;

    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_build_credits)
    values (input_buyer_id, input_catalog_item_id, item.price_build_credits);

    return buyer_balance;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint, bigint) from public, anon, authenticated;
