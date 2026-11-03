-- Production's public schema after migration 0056_drop_game_remixed_from, written by scripts/dump_database_schema.sh.
--
-- PostgreSQL database dump
--

\restrict kubenew

-- Dumped from database version 17.6
-- Dumped by pg_dump version 17.11 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: public; Type: SCHEMA; Schema: -; Owner: -
--

CREATE SCHEMA public;


--
-- Name: SCHEMA public; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON SCHEMA public IS 'standard public schema';


--
-- Name: asset_type; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.asset_type AS ENUM (
    'Mesh',
    'Texture',
    'Sound',
    'StoredObject',
    'Material'
);


--
-- Name: data_type; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.data_type AS ENUM (
    'mesh_data',
    'texture_data',
    'sound_data'
);


--
-- Name: plan_id; Type: TYPE; Schema: public; Owner: -
--

CREATE TYPE public.plan_id AS ENUM (
    'pro',
    'max'
);


--
-- Name: change_k_coins(uuid, bigint, text, bigint, uuid); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid) RETURNS bigint
    LANGUAGE plpgsql
    AS $$
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
    from public.user_k_coin_ledger k_coin_ledger_row
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

  insert into public.user_k_coin_ledger (profile_id, amount, balance_after, reason, game_id, request_id)
  values (input_profile_id, input_amount, new_balance, input_reason, input_game_id, input_request_id);

  return new_balance;
end;
$$;


--
-- Name: claim_daily_reward(uuid, bigint, bigint, integer); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.claim_daily_reward(input_profile_id uuid, input_k_coins bigint, input_build_credits bigint, input_min_hours integer) RETURNS TABLE(k_coins bigint, build_credits bigint, granted boolean)
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: claim_running_player_session(uuid, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.claim_running_player_session(input_profile_id uuid, input_request_id text) RETURNS boolean
    LANGUAGE plpgsql
    AS $$
declare
  updated_count integer;
begin
  update public.running_player_sessions
  set connected_at = now()
  where profile_id = input_profile_id
    and request_id = input_request_id;

  get diagnostics updated_count = row_count;
  return updated_count > 0;
end;
$$;


--
-- Name: delete_running_player_session_unless_play_auth_changed(uuid, text, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) RETURNS void
    LANGUAGE plpgsql
    AS $$
begin
  -- Lock the same profile row as the website launch transaction so leave cleanup cannot race a new launch.
  perform 1
  from public.user_profiles
  where id = input_profile_id
  for update;

  -- Delete normal leaves and logout leaves, but do nothing if a newer Play launch replaced this connection's auth hash.
  delete from public.running_player_sessions
  where profile_id = input_profile_id
    and request_id = input_request_id
    and not exists (
      select 1
      from public.user_desktop_play_auth
      where profile_id = input_profile_id
        and play_auth_hash is not null
        and play_auth_hash <> input_play_auth_hash
    );
end;
$$;


--
-- Name: grant_invoice(uuid, text, bigint, bigint); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_k_coins bigint, input_build_credits bigint) RETURNS TABLE(k_coins bigint, build_credits bigint, granted boolean)
    LANGUAGE plpgsql
    AS $$
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


--
-- Name: handle_new_user(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.handle_new_user() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
begin
  insert into public.user_profiles (id, email, username)
  values (new.id, new.email, new.raw_user_meta_data->>'username');
  return new;
end;
$$;


--
-- Name: overwrite_existing_player_session_if_exists(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.overwrite_existing_player_session_if_exists() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  PERFORM pg_advisory_xact_lock(hashtext(NEW.profile_id::text)::bigint);

  DELETE FROM public.player_sessions
  WHERE profile_id = NEW.profile_id;

  RETURN NEW;
END;
$$;


--
-- Name: search_game_object_assets(text, public.asset_type, integer, integer); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer) RETURNS TABLE(asset_id bigint, version_num bigint, name text, created_at timestamp with time zone, asset_type public.asset_type)
    LANGUAGE sql STABLE
    SET search_path TO ''
    AS $$
    select latest.asset_id, latest.version_num, latest.name, latest.created_at, latest.asset_type
    from (
        select distinct on (asset.asset_id) asset.* from public.game_object_assets asset
        order by asset.asset_id, asset.version_num desc
    ) latest
    where (input_asset_type is null or latest.asset_type = input_asset_type)
      and strpos(lower(latest.name), lower(input_query)) > 0
    order by latest.created_at desc, latest.asset_id desc
    offset greatest(input_offset, 0) limit least(greatest(input_limit, 0), 100);
$$;


--
-- Name: set_asset_version(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.set_asset_version() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path TO ''
    AS $$
declare original public.game_object_assets;
begin
    perform pg_advisory_xact_lock(hashtextextended('game_object_assets:' || new.asset_id::text, 0));
    select * into original from public.game_object_assets where asset_id = new.asset_id order by version_num limit 1;
    if found and (original.uploaded_by <> new.uploaded_by or original.asset_type <> new.asset_type) then
        raise exception 'Asset publisher and type cannot change';
    end if;
    select coalesce(max(version_num), 0) + 1 into new.version_num
    from public.game_object_assets where asset_id = new.asset_id;
    return new;
end;
$$;


--
-- Name: set_gameworld_version(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.set_gameworld_version() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
begin
  -- Serialize concurrent inserts for the same game so two saves cannot compute the same version.
  perform pg_advisory_xact_lock(new.game_id);
  new.version_num := coalesce(
    (select max(game_world_row.version_num) + 1 from public.game_worlds game_world_row where game_world_row.game_id = new.game_id),
    1
  );
  return new;
end;
$$;


--
-- Name: set_type_id(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.set_type_id() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path TO ''
    AS $$
begin
    if new.data_id is null then
        perform pg_advisory_xact_lock(hashtext(new.data_type::text));

        select coalesce(max(data_row.data_id), 0) + 1
        into new.data_id
        from public.data_metadata data_row
        where data_row.data_type = new.data_type;
    end if;

    return new;
end;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: blender_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.blender_metadata (
    blender_id bigint NOT NULL,
    uploaded_by uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT blender_metadata_blender_id_check CHECK ((blender_id > 0))
);


--
-- Name: blender_metadata_blender_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.blender_metadata ALTER COLUMN blender_id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.blender_metadata_blender_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: chat_messages; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_messages (
    game_id bigint NOT NULL,
    seq bigint NOT NULL,
    message jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: chat_usage; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.chat_usage (
    id bigint NOT NULL,
    profile_id uuid NOT NULL,
    game_id bigint,
    model_id text NOT NULL,
    input_tokens integer DEFAULT 0 NOT NULL,
    cache_write_tokens integer DEFAULT 0 NOT NULL,
    cache_read_tokens integer DEFAULT 0 NOT NULL,
    output_tokens integer,
    aws_request_id text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    credits_charged integer NOT NULL,
    message_id text
);


--
-- Name: chat_usage_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.chat_usage ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.chat_usage_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: data_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.data_metadata (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    data_type public.data_type NOT NULL,
    data_id bigint NOT NULL,
    uploaded_by uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: editor_share_auth; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.editor_share_auth (
    game_id bigint NOT NULL,
    invite_token text
);


--
-- Name: game_chats; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_chats (
    game_chat_id bigint NOT NULL,
    serverid text NOT NULL,
    time_live_server timestamp with time zone NOT NULL,
    userid text NOT NULL,
    message text NOT NULL
);


--
-- Name: game_chats_game_chat_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.game_chats ALTER COLUMN game_chat_id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.game_chats_game_chat_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: game_editor_worlds; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_editor_worlds (
    game_id bigint NOT NULL,
    gameworld_json text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: game_metadata; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_metadata (
    game_id bigint NOT NULL,
    name text NOT NULL,
    description text,
    creator_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now(),
    is_published boolean DEFAULT false,
    max_players integer NOT NULL,
    total_visits bigint DEFAULT 0 NOT NULL
);


--
-- Name: game_metadata_game_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.game_metadata ALTER COLUMN game_id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.game_metadata_game_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: game_object_assets; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_object_assets (
    asset_id bigint NOT NULL,
    version_num bigint NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    uploaded_by uuid NOT NULL,
    asset_type public.asset_type NOT NULL,
    data_json jsonb NOT NULL,
    CONSTRAINT game_object_assets_data_type_check CHECK (((jsonb_typeof(data_json) = 'object'::text) AND (data_json ? (asset_type)::text) AND ((data_json - (asset_type)::text) = '{}'::jsonb))),
    CONSTRAINT game_object_assets_name_check CHECK ((length(btrim(name)) > 0))
);


--
-- Name: game_object_assets_asset_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.game_object_assets ALTER COLUMN asset_id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.game_object_assets_asset_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: game_our_picks; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_our_picks (
    game_id bigint NOT NULL,
    our_picks_order double precision NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: game_worlds; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.game_worlds (
    created_at timestamp with time zone DEFAULT now(),
    game_id bigint NOT NULL,
    gameworld_json text NOT NULL,
    version_num bigint NOT NULL
);


--
-- Name: game_worlds_version_num_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.game_worlds ALTER COLUMN version_num ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.game_worlds_version_num_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: user_k_coin_ledger; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_k_coin_ledger (
    id bigint NOT NULL,
    profile_id uuid NOT NULL,
    amount bigint NOT NULL,
    balance_after bigint NOT NULL,
    reason text NOT NULL,
    game_id bigint,
    request_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT k_coin_ledger_amount_not_zero CHECK ((amount <> 0))
);


--
-- Name: k_coin_ledger_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.user_k_coin_ledger ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.k_coin_ledger_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: running_editor_servers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.running_editor_servers (
    request_id text NOT NULL,
    game_id bigint NOT NULL,
    editor_url text NOT NULL,
    public_ip text NOT NULL,
    playtest_external_port integer NOT NULL,
    CONSTRAINT running_editor_servers_playtest_external_port_check CHECK (((playtest_external_port >= 1) AND (playtest_external_port <= 65535)))
);


--
-- Name: running_player_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.running_player_sessions (
    profile_id uuid NOT NULL,
    request_id text NOT NULL,
    reserved_at timestamp with time zone DEFAULT now() NOT NULL,
    connected_at timestamp with time zone
);


--
-- Name: running_servers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.running_servers (
    request_id text NOT NULL,
    game_id bigint NOT NULL,
    gameworld_version_num bigint NOT NULL,
    max_players integer NOT NULL,
    public_ip text NOT NULL,
    external_port integer NOT NULL
);


--
-- Name: starting_editor_servers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.starting_editor_servers (
    request_id text NOT NULL,
    game_id bigint NOT NULL,
    starting_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: starting_player_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.starting_player_sessions (
    profile_id uuid NOT NULL,
    request_id text NOT NULL
);


--
-- Name: starting_servers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.starting_servers (
    request_id text NOT NULL,
    game_id bigint NOT NULL,
    gameworld_version_num bigint NOT NULL,
    max_players integer NOT NULL,
    starting_at timestamp with time zone NOT NULL
);


--
-- Name: user_desktop_play_auth; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_desktop_play_auth (
    profile_id uuid NOT NULL,
    play_auth_hash text
);


--
-- Name: user_likes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_likes (
    game_id bigint NOT NULL,
    profile_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: user_profiles; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_profiles (
    id uuid NOT NULL,
    email text,
    created_at timestamp with time zone DEFAULT now(),
    public_id bigint NOT NULL,
    username text NOT NULL,
    k_coins bigint DEFAULT 0 NOT NULL,
    build_credits bigint DEFAULT 0 NOT NULL,
    daily_reward_claimed_at timestamp with time zone,
    last_granted_invoice_id text,
    CONSTRAINT user_profiles_k_coins_not_negative CHECK ((k_coins >= 0)),
    CONSTRAINT user_profiles_username_characters CHECK ((username ~ '^[A-Za-z0-9]+$'::text)),
    CONSTRAINT user_profiles_username_length CHECK (((char_length(username) >= 4) AND (char_length(username) <= 16)))
);


--
-- Name: user_profiles_public_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.user_profiles ALTER COLUMN public_id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.user_profiles_public_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: user_subscriptions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.user_subscriptions (
    profile_id uuid NOT NULL,
    plan public.plan_id NOT NULL,
    status text NOT NULL,
    stripe_customer_id text NOT NULL,
    current_period_end timestamp with time zone NOT NULL
);


--
-- Name: blender_metadata blender_metadata_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.blender_metadata
    ADD CONSTRAINT blender_metadata_pkey PRIMARY KEY (blender_id);


--
-- Name: chat_messages chat_messages_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_messages
    ADD CONSTRAINT chat_messages_pkey PRIMARY KEY (game_id, seq);


--
-- Name: chat_usage chat_usage_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_usage
    ADD CONSTRAINT chat_usage_pkey PRIMARY KEY (id);


--
-- Name: data_metadata data_metadata_data_type_data_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_metadata
    ADD CONSTRAINT data_metadata_data_type_data_id_key UNIQUE (data_type, data_id);


--
-- Name: data_metadata data_metadata_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_metadata
    ADD CONSTRAINT data_metadata_pkey PRIMARY KEY (id);


--
-- Name: editor_share_auth editor_share_auth_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.editor_share_auth
    ADD CONSTRAINT editor_share_auth_pkey PRIMARY KEY (game_id);


--
-- Name: game_chats game_chats_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_chats
    ADD CONSTRAINT game_chats_pkey PRIMARY KEY (game_chat_id);


--
-- Name: game_editor_worlds game_editor_worlds_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_editor_worlds
    ADD CONSTRAINT game_editor_worlds_pkey PRIMARY KEY (game_id);


--
-- Name: game_metadata game_metadata_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_metadata
    ADD CONSTRAINT game_metadata_pkey PRIMARY KEY (game_id);


--
-- Name: game_object_assets game_object_assets_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_object_assets
    ADD CONSTRAINT game_object_assets_pkey PRIMARY KEY (asset_id, version_num);


--
-- Name: game_worlds game_worlds_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_worlds
    ADD CONSTRAINT game_worlds_pkey PRIMARY KEY (game_id, version_num);


--
-- Name: user_k_coin_ledger k_coin_ledger_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_k_coin_ledger
    ADD CONSTRAINT k_coin_ledger_pkey PRIMARY KEY (id);


--
-- Name: user_k_coin_ledger k_coin_ledger_request_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_k_coin_ledger
    ADD CONSTRAINT k_coin_ledger_request_id_key UNIQUE (request_id);


--
-- Name: game_our_picks our_picks_games_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_our_picks
    ADD CONSTRAINT our_picks_games_pkey PRIMARY KEY (game_id);


--
-- Name: user_desktop_play_auth player_auth_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_desktop_play_auth
    ADD CONSTRAINT player_auth_pkey PRIMARY KEY (profile_id);


--
-- Name: running_editor_servers running_editor_servers_game_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_editor_servers
    ADD CONSTRAINT running_editor_servers_game_id_key UNIQUE (game_id);


--
-- Name: running_editor_servers running_editor_servers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_editor_servers
    ADD CONSTRAINT running_editor_servers_pkey PRIMARY KEY (request_id);


--
-- Name: running_player_sessions running_player_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_player_sessions
    ADD CONSTRAINT running_player_sessions_pkey PRIMARY KEY (profile_id);


--
-- Name: running_servers running_servers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_servers
    ADD CONSTRAINT running_servers_pkey PRIMARY KEY (request_id);


--
-- Name: starting_editor_servers starting_editor_servers_game_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_editor_servers
    ADD CONSTRAINT starting_editor_servers_game_id_key UNIQUE (game_id);


--
-- Name: starting_editor_servers starting_editor_servers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_editor_servers
    ADD CONSTRAINT starting_editor_servers_pkey PRIMARY KEY (request_id);


--
-- Name: starting_player_sessions starting_player_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_player_sessions
    ADD CONSTRAINT starting_player_sessions_pkey PRIMARY KEY (profile_id);


--
-- Name: starting_servers starting_servers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_servers
    ADD CONSTRAINT starting_servers_pkey PRIMARY KEY (request_id);


--
-- Name: user_likes user_likes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_likes
    ADD CONSTRAINT user_likes_pkey PRIMARY KEY (game_id, profile_id);


--
-- Name: user_profiles user_profiles_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_pkey PRIMARY KEY (id);


--
-- Name: user_profiles user_profiles_public_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_profiles
    ADD CONSTRAINT user_profiles_public_id_key UNIQUE (public_id);


--
-- Name: user_subscriptions user_subscriptions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_subscriptions
    ADD CONSTRAINT user_subscriptions_pkey PRIMARY KEY (profile_id);


--
-- Name: user_subscriptions user_subscriptions_stripe_customer_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_subscriptions
    ADD CONSTRAINT user_subscriptions_stripe_customer_id_key UNIQUE (stripe_customer_id);


--
-- Name: chat_usage_aws_request_id_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX chat_usage_aws_request_id_key ON public.chat_usage USING btree (aws_request_id);


--
-- Name: chat_usage_profile_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX chat_usage_profile_created_idx ON public.chat_usage USING btree (profile_id, created_at);


--
-- Name: game_chats_serverid_time_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX game_chats_serverid_time_idx ON public.game_chats USING btree (serverid, time_live_server);


--
-- Name: game_chats_userid_time_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX game_chats_userid_time_idx ON public.game_chats USING btree (userid, time_live_server);


--
-- Name: game_object_assets_uploaded_by_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX game_object_assets_uploaded_by_idx ON public.game_object_assets USING btree (uploaded_by);


--
-- Name: k_coin_ledger_profile_created_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX k_coin_ledger_profile_created_idx ON public.user_k_coin_ledger USING btree (profile_id, created_at);


--
-- Name: our_picks_games_order_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX our_picks_games_order_idx ON public.game_our_picks USING btree (our_picks_order);


--
-- Name: running_player_sessions_unconnected_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX running_player_sessions_unconnected_idx ON public.running_player_sessions USING btree (request_id, reserved_at) WHERE (connected_at IS NULL);


--
-- Name: user_likes_profile_id_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX user_likes_profile_id_idx ON public.user_likes USING btree (profile_id);


--
-- Name: user_profiles_username_lower_key; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX user_profiles_username_lower_key ON public.user_profiles USING btree (lower(username));


--
-- Name: data_metadata data_metadata_set_type_id; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER data_metadata_set_type_id BEFORE INSERT ON public.data_metadata FOR EACH ROW EXECUTE FUNCTION public.set_type_id();


--
-- Name: game_object_assets game_object_assets_set_version; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER game_object_assets_set_version BEFORE INSERT ON public.game_object_assets FOR EACH ROW EXECUTE FUNCTION public.set_asset_version();


--
-- Name: game_worlds game_worlds_set_version_num; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER game_worlds_set_version_num BEFORE INSERT ON public.game_worlds FOR EACH ROW EXECUTE FUNCTION public.set_gameworld_version();


--
-- Name: blender_metadata blender_metadata_uploaded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.blender_metadata
    ADD CONSTRAINT blender_metadata_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES public.user_profiles(id);


--
-- Name: chat_messages chat_messages_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_messages
    ADD CONSTRAINT chat_messages_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: chat_usage chat_usage_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_usage
    ADD CONSTRAINT chat_usage_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE SET NULL;


--
-- Name: chat_usage chat_usage_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.chat_usage
    ADD CONSTRAINT chat_usage_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id) ON DELETE CASCADE;


--
-- Name: data_metadata data_metadata_uploaded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.data_metadata
    ADD CONSTRAINT data_metadata_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES public.user_profiles(id);


--
-- Name: editor_share_auth editor_share_auth_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.editor_share_auth
    ADD CONSTRAINT editor_share_auth_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: game_editor_worlds game_editor_worlds_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_editor_worlds
    ADD CONSTRAINT game_editor_worlds_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: game_metadata game_metadata_creator_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_metadata
    ADD CONSTRAINT game_metadata_creator_id_fkey FOREIGN KEY (creator_id) REFERENCES public.user_profiles(id);


--
-- Name: game_object_assets game_object_assets_uploaded_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_object_assets
    ADD CONSTRAINT game_object_assets_uploaded_by_fkey FOREIGN KEY (uploaded_by) REFERENCES public.user_profiles(id);


--
-- Name: user_k_coin_ledger k_coin_ledger_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_k_coin_ledger
    ADD CONSTRAINT k_coin_ledger_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE SET NULL;


--
-- Name: user_k_coin_ledger k_coin_ledger_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_k_coin_ledger
    ADD CONSTRAINT k_coin_ledger_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id) ON DELETE CASCADE;


--
-- Name: game_our_picks our_picks_games_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.game_our_picks
    ADD CONSTRAINT our_picks_games_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: user_desktop_play_auth player_auth_user_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_desktop_play_auth
    ADD CONSTRAINT player_auth_user_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id);


--
-- Name: running_editor_servers running_editor_servers_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_editor_servers
    ADD CONSTRAINT running_editor_servers_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: running_player_sessions running_player_sessions_request_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_player_sessions
    ADD CONSTRAINT running_player_sessions_request_id_fkey FOREIGN KEY (request_id) REFERENCES public.running_servers(request_id) ON DELETE CASCADE;


--
-- Name: running_player_sessions running_player_sessions_user_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_player_sessions
    ADD CONSTRAINT running_player_sessions_user_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id);


--
-- Name: running_servers running_servers_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_servers
    ADD CONSTRAINT running_servers_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id);


--
-- Name: running_servers running_servers_game_world_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.running_servers
    ADD CONSTRAINT running_servers_game_world_fkey FOREIGN KEY (game_id, gameworld_version_num) REFERENCES public.game_worlds(game_id, version_num);


--
-- Name: starting_editor_servers starting_editor_servers_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_editor_servers
    ADD CONSTRAINT starting_editor_servers_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: starting_player_sessions starting_player_sessions_request_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_player_sessions
    ADD CONSTRAINT starting_player_sessions_request_id_fkey FOREIGN KEY (request_id) REFERENCES public.starting_servers(request_id) ON DELETE CASCADE;


--
-- Name: starting_player_sessions starting_player_sessions_user_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_player_sessions
    ADD CONSTRAINT starting_player_sessions_user_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id);


--
-- Name: starting_servers starting_servers_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_servers
    ADD CONSTRAINT starting_servers_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id);


--
-- Name: starting_servers starting_servers_game_world_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.starting_servers
    ADD CONSTRAINT starting_servers_game_world_fkey FOREIGN KEY (game_id, gameworld_version_num) REFERENCES public.game_worlds(game_id, version_num);


--
-- Name: user_likes user_likes_game_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_likes
    ADD CONSTRAINT user_likes_game_id_fkey FOREIGN KEY (game_id) REFERENCES public.game_metadata(game_id) ON DELETE CASCADE;


--
-- Name: user_likes user_likes_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_likes
    ADD CONSTRAINT user_likes_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id) ON DELETE CASCADE;


--
-- Name: user_subscriptions user_subscriptions_profile_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.user_subscriptions
    ADD CONSTRAINT user_subscriptions_profile_id_fkey FOREIGN KEY (profile_id) REFERENCES public.user_profiles(id) ON DELETE CASCADE;


--
-- Name: game_object_assets Assets are public; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Assets are public" ON public.game_object_assets FOR SELECT TO authenticated, anon USING (true);


--
-- Name: blender_metadata Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.blender_metadata TO service_role USING (true) WITH CHECK (true);


--
-- Name: chat_messages Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.chat_messages TO service_role USING (true) WITH CHECK (true);


--
-- Name: chat_usage Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.chat_usage TO service_role USING (true) WITH CHECK (true);


--
-- Name: data_metadata Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.data_metadata TO service_role;


--
-- Name: editor_share_auth Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.editor_share_auth TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_chats Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_chats TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_editor_worlds Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_editor_worlds TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_metadata Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_metadata TO service_role;


--
-- Name: game_object_assets Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_object_assets TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_our_picks Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_our_picks TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_worlds Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.game_worlds TO service_role;


--
-- Name: running_editor_servers Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.running_editor_servers TO service_role USING (true) WITH CHECK (true);


--
-- Name: running_player_sessions Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.running_player_sessions TO service_role;


--
-- Name: running_servers Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.running_servers TO service_role;


--
-- Name: starting_editor_servers Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.starting_editor_servers TO service_role USING (true) WITH CHECK (true);


--
-- Name: starting_player_sessions Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.starting_player_sessions TO service_role;


--
-- Name: starting_servers Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.starting_servers TO service_role;


--
-- Name: user_desktop_play_auth Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.user_desktop_play_auth TO service_role USING (true) WITH CHECK (true);


--
-- Name: user_k_coin_ledger Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.user_k_coin_ledger TO service_role USING (true) WITH CHECK (true);


--
-- Name: user_likes Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.user_likes TO service_role USING (true) WITH CHECK (true);


--
-- Name: user_subscriptions Service role only; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Service role only" ON public.user_subscriptions TO service_role USING (true) WITH CHECK (true);


--
-- Name: game_metadata Users can read own games; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Users can read own games" ON public.game_metadata FOR SELECT USING ((auth.uid() = creator_id));


--
-- Name: user_profiles Users can read own profile; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Users can read own profile" ON public.user_profiles FOR SELECT USING ((auth.uid() = id));


--
-- Name: running_player_sessions Users can read own sessions; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Users can read own sessions" ON public.running_player_sessions FOR SELECT;


--
-- Name: starting_player_sessions Users can read own sessions; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY "Users can read own sessions" ON public.starting_player_sessions FOR SELECT;


--
-- Name: blender_metadata; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.blender_metadata ENABLE ROW LEVEL SECURITY;

--
-- Name: chat_messages; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.chat_messages ENABLE ROW LEVEL SECURITY;

--
-- Name: chat_usage; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.chat_usage ENABLE ROW LEVEL SECURITY;

--
-- Name: data_metadata; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.data_metadata ENABLE ROW LEVEL SECURITY;

--
-- Name: editor_share_auth; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.editor_share_auth ENABLE ROW LEVEL SECURITY;

--
-- Name: game_chats; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_chats ENABLE ROW LEVEL SECURITY;

--
-- Name: game_editor_worlds; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_editor_worlds ENABLE ROW LEVEL SECURITY;

--
-- Name: game_metadata; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_metadata ENABLE ROW LEVEL SECURITY;

--
-- Name: game_object_assets; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_object_assets ENABLE ROW LEVEL SECURITY;

--
-- Name: game_our_picks; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_our_picks ENABLE ROW LEVEL SECURITY;

--
-- Name: game_worlds; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.game_worlds ENABLE ROW LEVEL SECURITY;

--
-- Name: running_editor_servers; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.running_editor_servers ENABLE ROW LEVEL SECURITY;

--
-- Name: running_player_sessions; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.running_player_sessions ENABLE ROW LEVEL SECURITY;

--
-- Name: running_servers; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.running_servers ENABLE ROW LEVEL SECURITY;

--
-- Name: starting_editor_servers; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.starting_editor_servers ENABLE ROW LEVEL SECURITY;

--
-- Name: starting_player_sessions; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.starting_player_sessions ENABLE ROW LEVEL SECURITY;

--
-- Name: starting_servers; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.starting_servers ENABLE ROW LEVEL SECURITY;

--
-- Name: user_desktop_play_auth; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_desktop_play_auth ENABLE ROW LEVEL SECURITY;

--
-- Name: user_k_coin_ledger; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_k_coin_ledger ENABLE ROW LEVEL SECURITY;

--
-- Name: user_likes; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_likes ENABLE ROW LEVEL SECURITY;

--
-- Name: user_profiles; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_profiles ENABLE ROW LEVEL SECURITY;

--
-- Name: user_subscriptions; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.user_subscriptions ENABLE ROW LEVEL SECURITY;

--
-- Name: SCHEMA public; Type: ACL; Schema: -; Owner: -
--

GRANT USAGE ON SCHEMA public TO postgres;
GRANT USAGE ON SCHEMA public TO anon;
GRANT USAGE ON SCHEMA public TO authenticated;
GRANT USAGE ON SCHEMA public TO service_role;


--
-- Name: FUNCTION change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid) FROM PUBLIC;
GRANT ALL ON FUNCTION public.change_k_coins(input_profile_id uuid, input_amount bigint, input_reason text, input_game_id bigint, input_request_id uuid) TO service_role;


--
-- Name: FUNCTION claim_daily_reward(input_profile_id uuid, input_k_coins bigint, input_build_credits bigint, input_min_hours integer); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.claim_daily_reward(input_profile_id uuid, input_k_coins bigint, input_build_credits bigint, input_min_hours integer) FROM PUBLIC;
GRANT ALL ON FUNCTION public.claim_daily_reward(input_profile_id uuid, input_k_coins bigint, input_build_credits bigint, input_min_hours integer) TO service_role;


--
-- Name: FUNCTION claim_running_player_session(input_profile_id uuid, input_request_id text); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.claim_running_player_session(input_profile_id uuid, input_request_id text) FROM PUBLIC;
GRANT ALL ON FUNCTION public.claim_running_player_session(input_profile_id uuid, input_request_id text) TO anon;
GRANT ALL ON FUNCTION public.claim_running_player_session(input_profile_id uuid, input_request_id text) TO authenticated;
GRANT ALL ON FUNCTION public.claim_running_player_session(input_profile_id uuid, input_request_id text) TO service_role;


--
-- Name: FUNCTION delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) FROM PUBLIC;
GRANT ALL ON FUNCTION public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) TO anon;
GRANT ALL ON FUNCTION public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) TO authenticated;
GRANT ALL ON FUNCTION public.delete_running_player_session_unless_play_auth_changed(input_profile_id uuid, input_request_id text, input_play_auth_hash text) TO service_role;


--
-- Name: FUNCTION grant_invoice(input_profile_id uuid, input_invoice_id text, input_k_coins bigint, input_build_credits bigint); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_k_coins bigint, input_build_credits bigint) FROM PUBLIC;
GRANT ALL ON FUNCTION public.grant_invoice(input_profile_id uuid, input_invoice_id text, input_k_coins bigint, input_build_credits bigint) TO service_role;


--
-- Name: FUNCTION handle_new_user(); Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON FUNCTION public.handle_new_user() TO anon;
GRANT ALL ON FUNCTION public.handle_new_user() TO authenticated;
GRANT ALL ON FUNCTION public.handle_new_user() TO service_role;


--
-- Name: FUNCTION overwrite_existing_player_session_if_exists(); Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON FUNCTION public.overwrite_existing_player_session_if_exists() TO anon;
GRANT ALL ON FUNCTION public.overwrite_existing_player_session_if_exists() TO authenticated;
GRANT ALL ON FUNCTION public.overwrite_existing_player_session_if_exists() TO service_role;


--
-- Name: FUNCTION search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer); Type: ACL; Schema: public; Owner: -
--

REVOKE ALL ON FUNCTION public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer) FROM PUBLIC;
GRANT ALL ON FUNCTION public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer) TO anon;
GRANT ALL ON FUNCTION public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer) TO authenticated;
GRANT ALL ON FUNCTION public.search_game_object_assets(input_query text, input_asset_type public.asset_type, input_offset integer, input_limit integer) TO service_role;


--
-- Name: FUNCTION set_asset_version(); Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON FUNCTION public.set_asset_version() TO anon;
GRANT ALL ON FUNCTION public.set_asset_version() TO authenticated;
GRANT ALL ON FUNCTION public.set_asset_version() TO service_role;


--
-- Name: FUNCTION set_gameworld_version(); Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON FUNCTION public.set_gameworld_version() TO anon;
GRANT ALL ON FUNCTION public.set_gameworld_version() TO authenticated;
GRANT ALL ON FUNCTION public.set_gameworld_version() TO service_role;


--
-- Name: FUNCTION set_type_id(); Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON FUNCTION public.set_type_id() TO anon;
GRANT ALL ON FUNCTION public.set_type_id() TO authenticated;
GRANT ALL ON FUNCTION public.set_type_id() TO service_role;


--
-- Name: TABLE blender_metadata; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.blender_metadata TO service_role;


--
-- Name: SEQUENCE blender_metadata_blender_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.blender_metadata_blender_id_seq TO anon;
GRANT ALL ON SEQUENCE public.blender_metadata_blender_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.blender_metadata_blender_id_seq TO service_role;


--
-- Name: TABLE chat_messages; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.chat_messages TO anon;
GRANT ALL ON TABLE public.chat_messages TO authenticated;
GRANT ALL ON TABLE public.chat_messages TO service_role;


--
-- Name: TABLE chat_usage; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.chat_usage TO anon;
GRANT ALL ON TABLE public.chat_usage TO authenticated;
GRANT ALL ON TABLE public.chat_usage TO service_role;


--
-- Name: SEQUENCE chat_usage_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.chat_usage_id_seq TO anon;
GRANT ALL ON SEQUENCE public.chat_usage_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.chat_usage_id_seq TO service_role;


--
-- Name: TABLE data_metadata; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.data_metadata TO anon;
GRANT ALL ON TABLE public.data_metadata TO authenticated;
GRANT ALL ON TABLE public.data_metadata TO service_role;


--
-- Name: TABLE editor_share_auth; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.editor_share_auth TO anon;
GRANT ALL ON TABLE public.editor_share_auth TO authenticated;
GRANT ALL ON TABLE public.editor_share_auth TO service_role;


--
-- Name: TABLE game_chats; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_chats TO anon;
GRANT ALL ON TABLE public.game_chats TO authenticated;
GRANT ALL ON TABLE public.game_chats TO service_role;


--
-- Name: SEQUENCE game_chats_game_chat_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.game_chats_game_chat_id_seq TO anon;
GRANT ALL ON SEQUENCE public.game_chats_game_chat_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.game_chats_game_chat_id_seq TO service_role;


--
-- Name: TABLE game_editor_worlds; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_editor_worlds TO anon;
GRANT ALL ON TABLE public.game_editor_worlds TO authenticated;
GRANT ALL ON TABLE public.game_editor_worlds TO service_role;


--
-- Name: TABLE game_metadata; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_metadata TO anon;
GRANT ALL ON TABLE public.game_metadata TO authenticated;
GRANT ALL ON TABLE public.game_metadata TO service_role;


--
-- Name: SEQUENCE game_metadata_game_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.game_metadata_game_id_seq TO anon;
GRANT ALL ON SEQUENCE public.game_metadata_game_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.game_metadata_game_id_seq TO service_role;


--
-- Name: TABLE game_object_assets; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_object_assets TO service_role;
GRANT SELECT ON TABLE public.game_object_assets TO anon;
GRANT SELECT ON TABLE public.game_object_assets TO authenticated;


--
-- Name: SEQUENCE game_object_assets_asset_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.game_object_assets_asset_id_seq TO anon;
GRANT ALL ON SEQUENCE public.game_object_assets_asset_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.game_object_assets_asset_id_seq TO service_role;


--
-- Name: TABLE game_our_picks; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_our_picks TO anon;
GRANT ALL ON TABLE public.game_our_picks TO authenticated;
GRANT ALL ON TABLE public.game_our_picks TO service_role;


--
-- Name: TABLE game_worlds; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.game_worlds TO anon;
GRANT ALL ON TABLE public.game_worlds TO authenticated;
GRANT ALL ON TABLE public.game_worlds TO service_role;


--
-- Name: SEQUENCE game_worlds_version_num_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.game_worlds_version_num_seq TO anon;
GRANT ALL ON SEQUENCE public.game_worlds_version_num_seq TO authenticated;
GRANT ALL ON SEQUENCE public.game_worlds_version_num_seq TO service_role;


--
-- Name: TABLE user_k_coin_ledger; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.user_k_coin_ledger TO anon;
GRANT ALL ON TABLE public.user_k_coin_ledger TO authenticated;
GRANT ALL ON TABLE public.user_k_coin_ledger TO service_role;


--
-- Name: SEQUENCE k_coin_ledger_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.k_coin_ledger_id_seq TO anon;
GRANT ALL ON SEQUENCE public.k_coin_ledger_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.k_coin_ledger_id_seq TO service_role;


--
-- Name: TABLE running_editor_servers; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.running_editor_servers TO anon;
GRANT ALL ON TABLE public.running_editor_servers TO authenticated;
GRANT ALL ON TABLE public.running_editor_servers TO service_role;


--
-- Name: TABLE running_player_sessions; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.running_player_sessions TO anon;
GRANT ALL ON TABLE public.running_player_sessions TO authenticated;
GRANT ALL ON TABLE public.running_player_sessions TO service_role;


--
-- Name: TABLE running_servers; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.running_servers TO anon;
GRANT ALL ON TABLE public.running_servers TO authenticated;
GRANT ALL ON TABLE public.running_servers TO service_role;


--
-- Name: TABLE starting_editor_servers; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.starting_editor_servers TO anon;
GRANT ALL ON TABLE public.starting_editor_servers TO authenticated;
GRANT ALL ON TABLE public.starting_editor_servers TO service_role;


--
-- Name: TABLE starting_player_sessions; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.starting_player_sessions TO anon;
GRANT ALL ON TABLE public.starting_player_sessions TO authenticated;
GRANT ALL ON TABLE public.starting_player_sessions TO service_role;


--
-- Name: TABLE starting_servers; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.starting_servers TO anon;
GRANT ALL ON TABLE public.starting_servers TO authenticated;
GRANT ALL ON TABLE public.starting_servers TO service_role;


--
-- Name: TABLE user_desktop_play_auth; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.user_desktop_play_auth TO anon;
GRANT ALL ON TABLE public.user_desktop_play_auth TO authenticated;
GRANT ALL ON TABLE public.user_desktop_play_auth TO service_role;


--
-- Name: TABLE user_likes; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.user_likes TO anon;
GRANT ALL ON TABLE public.user_likes TO authenticated;
GRANT ALL ON TABLE public.user_likes TO service_role;


--
-- Name: TABLE user_profiles; Type: ACL; Schema: public; Owner: -
--

GRANT SELECT,INSERT,REFERENCES,DELETE,TRIGGER,TRUNCATE,MAINTAIN ON TABLE public.user_profiles TO anon;
GRANT SELECT,INSERT,REFERENCES,DELETE,TRIGGER,TRUNCATE,MAINTAIN ON TABLE public.user_profiles TO authenticated;
GRANT ALL ON TABLE public.user_profiles TO service_role;


--
-- Name: SEQUENCE user_profiles_public_id_seq; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON SEQUENCE public.user_profiles_public_id_seq TO anon;
GRANT ALL ON SEQUENCE public.user_profiles_public_id_seq TO authenticated;
GRANT ALL ON SEQUENCE public.user_profiles_public_id_seq TO service_role;


--
-- Name: TABLE user_subscriptions; Type: ACL; Schema: public; Owner: -
--

GRANT ALL ON TABLE public.user_subscriptions TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR SEQUENCES; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON SEQUENCES TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR SEQUENCES; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON SEQUENCES TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON SEQUENCES TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON SEQUENCES TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON SEQUENCES TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR FUNCTIONS; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON FUNCTIONS TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON FUNCTIONS TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON FUNCTIONS TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON FUNCTIONS TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR FUNCTIONS; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON FUNCTIONS TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON FUNCTIONS TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON FUNCTIONS TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON FUNCTIONS TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR TABLES; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE postgres IN SCHEMA public GRANT ALL ON TABLES TO service_role;


--
-- Name: DEFAULT PRIVILEGES FOR TABLES; Type: DEFAULT ACL; Schema: public; Owner: -
--

ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON TABLES TO postgres;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON TABLES TO anon;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON TABLES TO authenticated;
ALTER DEFAULT PRIVILEGES FOR ROLE supabase_admin IN SCHEMA public GRANT ALL ON TABLES TO service_role;


--
-- PostgreSQL database dump complete
--

\unrestrict kubenew

