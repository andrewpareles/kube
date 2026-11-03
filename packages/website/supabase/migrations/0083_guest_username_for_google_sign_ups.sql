-- Google sign ups arrive with no username, so they get Guest-<user_id>. The hyphen keeps it out of reach of the sign up form,
-- and only the account with that user_id can hold it.
alter table public.user_profiles
drop constraint user_profiles_username_characters;

alter table public.user_profiles
add constraint user_profiles_username_characters
check (username ~ '^[A-Za-z0-9]+$' or username = 'Guest-' || user_id);

create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
declare
  new_user_id bigint := nextval('public.user_profiles_user_id_seq');
begin
  insert into public.user_profiles (id, email, user_id, username)
  values (new.id, new.email, new_user_id, coalesce(new.raw_user_meta_data->>'username', 'Guest-' || new_user_id));
  return new;
end;
$$;
