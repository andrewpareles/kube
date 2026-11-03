-- Everything is free for now. Every account can use the same number of credits of AI a day, counted from chat_usage, so
-- there is no balance to grant, refill or spend. Hats cost nothing, so they have no price.
drop function public.claim_daily_reward(uuid, bigint, integer);
drop function public.grant_invoice(uuid, text, bigint);
alter table public.user_profiles
    drop column credits,
    drop column daily_reward_claimed_at,
    drop column last_granted_invoice_id,
    drop column free_hats_left;
alter table public.catalog_metadata drop column price_in_credits;
alter table public.user_owned_catalog_items drop column price_paid_in_credits;

-- A model call is recorded once, however often it is sent, with everything it cost.
create or replace function public.charge_chat_usage(
    input_profile_id uuid,
    input_game_id bigint,
    input_message_id text,
    input_model_id text,
    input_input_tokens integer,
    input_cache_write_tokens integer,
    input_cache_read_tokens integer,
    input_output_tokens integer,
    input_aws_request_id text,
    input_credits bigint
)
returns void
language sql
set search_path = ''
as $$
    insert into public.chat_usage (
        message_id, profile_id, game_id, model_id, input_tokens, cache_write_tokens,
        cache_read_tokens, output_tokens, aws_request_id, credits_charged
    )
    values (
        input_message_id, input_profile_id, input_game_id, input_model_id, input_input_tokens,
        input_cache_write_tokens, input_cache_read_tokens, input_output_tokens, input_aws_request_id, input_credits
    )
    on conflict (aws_request_id) do nothing;
$$;

-- TODO: AI-PICKED-VALUE: A day starts at midnight UTC, the same moment for everyone.
create function public.credits_used_today(input_profile_id uuid)
returns bigint
language sql
stable
set search_path = ''
as $$
    select coalesce(sum(credits_charged), 0) from public.chat_usage
    where profile_id = input_profile_id and created_at >= date_trunc('day', now(), 'UTC');
$$;
revoke execute on function public.credits_used_today(uuid) from public, anon, authenticated;
grant execute on function public.credits_used_today(uuid) to service_role;

drop function public.buy_catalog_item(uuid, bigint, bigint);
create function public.buy_catalog_item(input_buyer_id uuid, input_catalog_item_id bigint) returns void
language plpgsql
set search_path to ''
as $$
begin
    perform 1 from public.catalog_metadata where catalog_item_id = input_catalog_item_id and is_listed for share;
    if not found then
        raise exception 'This item is not for sale.';
    end if;
    insert into public.user_owned_catalog_items (profile_id, catalog_item_id) values (input_buyer_id, input_catalog_item_id)
    on conflict do nothing;
    if not found then
        raise exception 'You already own this item.';
    end if;
end;
$$;
revoke execute on function public.buy_catalog_item(uuid, bigint) from public, anon, authenticated;

-- It names the dropped column in its body, so it is made again without it.
create or replace function public.publish_catalog_item(
    input_catalog_item_id bigint,
    input_creator_id uuid,
    input_name text,
    input_description text,
    input_is_listed boolean,
    input_mesh_data_ids bigint[]
)
returns public.catalog_metadata
language plpgsql
set search_path = ''
as $$
declare
    item public.catalog_metadata;
begin
    if (
        select count(*) from public.data_metadata
        where data_type = 'mesh_data' and uploaded_by = input_creator_id and data_id = any(input_mesh_data_ids)
    ) <> cardinality(input_mesh_data_ids) then
        raise sqlstate 'PT403' using message = 'Only meshes this game''s creator made can be published to the catalog.';
    end if;
    insert into public.catalog_metadata (catalog_item_id, creator_id, name, description, is_listed)
    values (input_catalog_item_id, input_creator_id, input_name, nullif(input_description, ''), input_is_listed)
    returning * into item;
    insert into public.user_owned_catalog_items (profile_id, catalog_item_id)
    values (input_creator_id, input_catalog_item_id);
    return item;
end;
$$;
