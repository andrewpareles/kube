-- Editors reach the database only through PostgREST, so each thing they do in one transaction is a function here. A function
-- raises sqlstate 'PT<status>' for an error meant for the user, and PostgREST answers with that HTTP status. A null
-- input_request_id is a dev editor, which has no deployment to check.

create function public.editor_load_world(input_game_id bigint, input_request_id text)
returns table (creator_id uuid, editor_world_uuid uuid, published_world_uuid uuid)
language plpgsql
set search_path = ''
as $$
begin
    if not exists (select 1 from public.game_metadata g where g.game_id = input_game_id) then
        raise sqlstate 'PT404' using message = 'Game not found.';
    end if;
    if input_request_id is not null and not exists (
        select 1 from public.starting_editor_servers s where s.request_id = input_request_id and s.game_id = input_game_id
        union all
        select 1 from public.running_editor_servers r where r.request_id = input_request_id and r.game_id = input_game_id
    ) then
        raise sqlstate 'PT410' using message = 'Editor server stopped. Reopen the editor.';
    end if;
    return query
    select
        g.creator_id,
        (select w.world_uuid from public.game_editor_worlds w where w.game_id = input_game_id),
        (select p.world_uuid from public.game_worlds p where p.game_id = input_game_id order by p.version_num desc limit 1)
    from public.game_metadata g
    where g.game_id = input_game_id;
end;
$$;

-- The game row lock keeps a save from racing the cleanup that removes its editor.
create function public.editor_save_world(input_game_id bigint, input_request_id text, input_world_uuid uuid)
returns void
language plpgsql
set search_path = ''
as $$
begin
    perform 1 from public.game_metadata where game_id = input_game_id for update;
    if input_request_id is not null and not exists (
        select 1 from public.starting_editor_servers where request_id = input_request_id and game_id = input_game_id
        union all
        select 1 from public.running_editor_servers where request_id = input_request_id and game_id = input_game_id
    ) then
        raise sqlstate 'PT410' using message = 'Editor server stopped. Reopen the editor.';
    end if;
    insert into public.game_editor_worlds (game_id, world_uuid) values (input_game_id, input_world_uuid)
    on conflict (game_id) do update set world_uuid = excluded.world_uuid, updated_at = now();
end;
$$;

-- Only this deployment's rows go, so a replacement editor keeps its own.
create function public.editor_remove_deployment(input_game_id bigint, input_request_id text)
returns void
language plpgsql
set search_path = ''
as $$
begin
    perform 1 from public.game_metadata where game_id = input_game_id for update;
    delete from public.starting_editor_servers where request_id = input_request_id;
    delete from public.running_editor_servers where request_id = input_request_id;
end;
$$;

-- Checked on every request, so a creator change or a revoked invite applies at once. Returns the creator, who pays for the
-- AI chat whoever is using it.
create function public.editor_browser_access(input_game_id bigint, input_request_id text, input_profile_id uuid, input_invite_token text)
returns uuid
language plpgsql
set search_path = ''
as $$
declare
    game_creator_id uuid;
begin
    select g.creator_id into game_creator_id
    from public.game_metadata g
    where g.game_id = input_game_id and (
        g.creator_id = input_profile_id
        or exists (
            select 1 from public.editor_share_auth s
            where s.game_id = g.game_id and s.invite_token = input_invite_token and s.invite_token <> ''
        )
    );
    if not found then
        raise sqlstate 'PT403' using message = 'You do not have access to this editor. The invite link may have been revoked.';
    end if;
    if input_request_id is not null and not exists (
        select 1 from public.running_editor_servers where request_id = input_request_id and game_id = input_game_id
    ) then
        raise sqlstate 'PT410' using message = 'Editor server stopped. Reopen the editor.';
    end if;
    return game_creator_id;
end;
$$;

create function public.record_asset_import(input_asset_id bigint, input_version_num bigint)
returns void
language sql
set search_path = ''
as $$
    update public.asset_metadata set import_count = import_count + 1 where asset_id = input_asset_id and version_num = input_version_num;
$$;

-- Only meshes the creator uploaded can be sold, so nobody sells a copy of someone else's model. Publishing an item also
-- gives it to its creator, for nothing.
create function public.publish_catalog_item(
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
    insert into public.user_owned_catalog_items (profile_id, catalog_item_id, price_paid_in_credits)
    values (input_creator_id, input_catalog_item_id, 0);
    return item;
end;
$$;

-- The game row lock numbers one turn's messages at a time. A user message that is already saved, because the turn resumes it,
-- is passed as null.
create function public.start_chat_reply(input_game_id bigint, input_user_message jsonb, input_reply jsonb)
returns void
language plpgsql
set search_path = ''
as $$
declare
    next_seq bigint;
begin
    perform 1 from public.game_metadata where game_id = input_game_id for update;
    select coalesce(max(seq), 0) + 1 into next_seq from public.chat_messages where game_id = input_game_id;
    if input_user_message is not null then
        insert into public.chat_messages (game_id, seq, message) values (input_game_id, next_seq, input_user_message);
        next_seq := next_seq + 1;
    end if;
    insert into public.chat_messages (game_id, seq, message) values (input_game_id, next_seq, input_reply);
end;
$$;

-- The model starts from the latest summary, so it needs only the messages from there on.
create function public.chat_messages_since_summary(input_game_id bigint)
returns setof jsonb
language sql
stable
set search_path = ''
as $$
    select message from public.chat_messages
    where game_id = input_game_id and seq >= coalesce((
        select max(seq) from public.chat_messages
        where game_id = input_game_id
            and jsonb_path_exists(message, '$.parts[*] ? (@.type == "data-compaction" && @.data.summary.type() == "string")')
    ), 0)
    order by seq;
$$;

-- A model call's usage row and its charge are one statement, so a call recorded twice is charged once, and a failed charge
-- leaves no row claiming it was paid. The balance stops at 0, and credits_charged records what was taken.
create function public.charge_chat_usage(
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
    with profile as (
        select credits from public.user_profiles where id = input_profile_id for update
    ),
    inserted as (
        insert into public.chat_usage (
            message_id, profile_id, game_id, model_id, input_tokens, cache_write_tokens,
            cache_read_tokens, output_tokens, aws_request_id, credits_charged
        )
        select
            input_message_id, input_profile_id, input_game_id, input_model_id, input_input_tokens,
            input_cache_write_tokens, input_cache_read_tokens, input_output_tokens, input_aws_request_id,
            least(input_credits, greatest(profile.credits, 0))
        from profile
        on conflict (aws_request_id) do nothing
        returning profile_id, credits_charged
    )
    update public.user_profiles set credits = user_profiles.credits - inserted.credits_charged
    from inserted where user_profiles.id = inserted.profile_id;
$$;

revoke execute on function
    public.editor_load_world(bigint, text),
    public.editor_save_world(bigint, text, uuid),
    public.editor_remove_deployment(bigint, text),
    public.editor_browser_access(bigint, text, uuid, text),
    public.record_asset_import(bigint, bigint),
    public.publish_catalog_item(bigint, uuid, text, text, boolean, bigint[]),
    public.start_chat_reply(bigint, jsonb, jsonb),
    public.chat_messages_since_summary(bigint),
    public.charge_chat_usage(uuid, bigint, text, text, integer, integer, integer, integer, text, bigint)
from public, anon, authenticated;
grant execute on function
    public.editor_load_world(bigint, text),
    public.editor_save_world(bigint, text, uuid),
    public.editor_remove_deployment(bigint, text),
    public.editor_browser_access(bigint, text, uuid, text),
    public.record_asset_import(bigint, bigint),
    public.publish_catalog_item(bigint, uuid, text, text, boolean, bigint[]),
    public.start_chat_reply(bigint, jsonb, jsonb),
    public.chat_messages_since_summary(bigint),
    public.charge_chat_usage(uuid, bigint, text, text, integer, integer, integer, integer, text, bigint)
to service_role;

revoke execute on function
    public.claim_running_player_session(uuid, text),
    public.delete_running_player_session_unless_play_auth_changed(uuid, text, text),
    public.delete_running_server_if_empty(text),
    public.search_asset_metadata(text, public.asset_type, integer, integer)
from anon, authenticated;

alter default privileges for role postgres revoke execute on functions from public;
alter default privileges for role postgres in schema public revoke execute on functions from anon, authenticated;
