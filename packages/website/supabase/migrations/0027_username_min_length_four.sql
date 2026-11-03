-- A username has to be at least four characters. Nobody holds a shorter one, so this only narrows
-- what can be taken from here on and leaves every account alone.
alter table public.user_profiles
drop constraint user_profiles_username_length;

alter table public.user_profiles
add constraint user_profiles_username_length
check (char_length(username) between 4 and 16);
