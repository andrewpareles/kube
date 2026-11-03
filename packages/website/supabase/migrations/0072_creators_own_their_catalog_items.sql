-- Publishing a catalog item now gives it to its creator. Creators of the items published before that get theirs here.
insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_k_coins)
select creator_id, catalog_item_id, 0 from public.catalog_metadata
on conflict (profile_id, catalog_item_id) do nothing;
