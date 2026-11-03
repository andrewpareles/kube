alter table public.game_likes rename to user_likes;
alter table public.profiles rename to user_profiles;
alter table public."uploads-metadata" rename to data_metadata;
alter table public.gameworlds rename to game_worlds;
alter table public."games-metadata" rename to game_metadata;

alter table public.game_metadata
rename column gameworld_version_num to world_version;

alter sequence public.profiles_public_id_seq rename to user_profiles_public_id_seq;
alter sequence public.games_metadata_game_id_seq rename to game_metadata_game_id_seq;
alter sequence public.gameworlds_version_num_seq rename to game_worlds_version_num_seq;

alter table public.user_profiles
rename constraint profiles_pkey to user_profiles_pkey;

alter table public.user_profiles
rename constraint profiles_public_id_key to user_profiles_public_id_key;

alter table public.user_profiles
rename constraint profiles_username_length to user_profiles_username_length;

alter table public.data_metadata
rename constraint uploads_pkey to data_metadata_pkey;

alter table public.data_metadata
rename constraint uploads_uploaded_by_fkey to data_metadata_uploaded_by_fkey;

alter table public.data_metadata
rename constraint uploads_data_type_data_id_key to data_metadata_data_type_data_id_key;

alter table public.game_worlds
rename constraint gameworlds_pkey to game_worlds_pkey;

alter table public.game_metadata
rename constraint games_pkey to game_metadata_pkey;

alter table public.game_metadata
rename constraint games_creator_id_fkey to game_metadata_creator_id_fkey;

alter table public.game_metadata
rename constraint games_gameworld_fkey to game_metadata_game_world_fkey;

alter table public.user_likes
rename constraint game_likes_pkey to user_likes_pkey;

alter table public.user_likes
rename constraint game_likes_game_id_fkey to user_likes_game_id_fkey;

alter table public.user_likes
rename constraint game_likes_profile_id_fkey to user_likes_profile_id_fkey;

alter index public.game_likes_profile_id_idx
rename to user_likes_profile_id_idx;

alter table public.starting_servers
rename constraint starting_servers_gameworld_fkey to starting_servers_game_world_fkey;

alter table public.running_servers
rename constraint running_servers_gameworld_fkey to running_servers_game_world_fkey;

alter table public.player_auth
rename constraint player_auth_profile_id_fkey to player_auth_user_profile_id_fkey;

alter table public.starting_player_sessions
rename constraint starting_player_sessions_profile_id_fkey to starting_player_sessions_user_profile_id_fkey;

alter table public.running_player_sessions
rename constraint running_player_sessions_profile_id_fkey to running_player_sessions_user_profile_id_fkey;

create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
begin
  insert into public.user_profiles (id, email)
  values (new.id, new.email);
  return new;
end;
$$;

create or replace function public.set_type_id()
returns trigger
language plpgsql
as $$
begin
    if new.data_id is null then
        perform pg_advisory_xact_lock(hashtext(new.data_type::text));

        select coalesce(max(data_row.data_id), 0) + 1
        into new.data_id
        from public.data_metadata data_row
        where data_row.data_type = new.data_type;
    end if;

    if new.url is null or new.url = '' then
        new.url := format('%s/%s', new.data_type::text, new.data_id::text);
    end if;

    return new;
end;
$$;

alter trigger uploads_set_type_id on public.data_metadata
rename to data_metadata_set_type_id;

create or replace function public.set_gameworld_version()
returns trigger
language plpgsql
as $$
begin
  new.version_num := coalesce(
    (select max(game_world_row.version_num) + 1 from public.game_worlds game_world_row where game_world_row.game_id = new.game_id),
    1
  );
  return new;
end;
$$;
