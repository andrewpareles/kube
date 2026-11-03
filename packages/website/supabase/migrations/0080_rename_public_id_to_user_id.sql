-- A profile's public number is its user id. Its uuid keeps the name id here and profile_id everywhere else.
alter table public.user_profiles rename column public_id to user_id;
alter sequence public.user_profiles_public_id_seq rename to user_profiles_user_id_seq;
alter table public.user_profiles rename constraint user_profiles_public_id_key to user_profiles_user_id_key;
