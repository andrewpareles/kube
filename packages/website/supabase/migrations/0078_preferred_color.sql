-- The color a player's character wears in games. It starts as the default character's blue, written as #rrggbb.
alter table public.user_profiles
add column preferred_color text not null default '#408ce6'
constraint user_profiles_preferred_color_is_hex check (preferred_color ~ '^#[0-9a-f]{6}$');
