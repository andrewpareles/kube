-- Every new account starts as Guest-<user_id> and picks its name later on its profile, so a name sent with the sign up is ignored.
create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
declare
  new_user_id bigint := nextval('public.user_profiles_user_id_seq');
begin
  insert into public.user_profiles (id, email, user_id, username)
  values (new.id, new.email, new_user_id, 'Guest-' || new_user_id);
  return new;
end;
$$;
