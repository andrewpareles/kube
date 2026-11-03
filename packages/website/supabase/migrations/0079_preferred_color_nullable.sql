-- No preferred color means the player hasn't picked one, and each game picks for them.
alter table public.user_profiles
alter column preferred_color drop not null,
alter column preferred_color drop default;

-- Nobody can pick a color yet, so every color saved so far is the old default.
update public.user_profiles set preferred_color = null where preferred_color = '#408ce6';
