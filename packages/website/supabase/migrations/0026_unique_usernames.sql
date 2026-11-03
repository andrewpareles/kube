-- A username is the one name an account is known by, and the table has always held two accounts
-- apart by it without regard to case, so @Andrew and @andrew cannot both exist. The index that does
-- that kept the name it had before the table stopped being called profiles, which is why it was
-- missed when everything else was renamed.
alter index public.profiles_username_lower_key
rename to user_profiles_username_lower_key;

-- Only one account can be called 'Guest' under that index, so the name the trigger below used to
-- hand out made sign up work exactly once and then fail for everybody after. That one account, if
-- it is there, needs a name of its own. public_id is already one per account, so user<public_id> is
-- too.
update public.user_profiles
set username = 'user' || public_id
where username = 'Guest';

-- Letters and digits only. Without this '@bob' and '@bob ' would be two accounts that look exactly
-- alike on the page, and the index above would let both exist.
alter table public.user_profiles
add constraint user_profiles_username_characters
check (username ~ '^[A-Za-z0-9]+$');

-- The name now comes from the sign up form, which hands it to the auth user as metadata. Nothing is
-- left to fall back on, because a name nobody picked could not be unique anyway: signing someone up
-- without one now fails outright rather than taking a name that is already spoken for.
alter table public.user_profiles
alter column username drop default;

create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
begin
  insert into public.user_profiles (id, email, username)
  values (new.id, new.email, new.raw_user_meta_data->>'username');
  return new;
end;
$$;
